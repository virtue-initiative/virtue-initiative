use std::any::Any;
use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::{
    AlertReason, Event, LifecycleKind, Observer, PartialStatus, ProcessStoppedReason, StateType,
    UploadKind,
};
use crate::platform::ScreenshotHooks;

pub(crate) const HIGH_RISK_LIFECYCLE_ALERT: f32 = 0.9;
pub(crate) const MEDIUM_RISK_LIFECYCLE_ALERT: f32 = 0.6;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub enum LifecycleStatus {
    #[default]
    Running,
    Suspended,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)] // Handle upgrades gracefully by defaulting missing fields
pub struct LifecycleObserverState {
    pub status: LifecycleStatus,

    pub last_login: i64,

    // Running
    pub last_process_stopped_other: i64,
    pub last_process_stopped_shutdown: i64,
    pub last_process_stopped_user: i64,
    pub last_computer_suspend: i64,
    pub last_computer_resume: i64,
    pub last_ping: i64,
    pub last_process_started: i64,
    pub last_running_started: i64,
    pub last_sent_boot: i64,

    // Suspended
    pub pings_while_suspended: i64,

    // User stop tracking
    pub user_stop_requested: bool,
}

pub struct LifecycleObserver<C = ()> {
    pub state: LifecycleObserverState,
    platform_hooks: Box<dyn ScreenshotHooks>,
    sender: Sender<Event<C>>,
}

impl<C: 'static> LifecycleObserver<C> {
    pub fn new(platform_hooks: Box<dyn ScreenshotHooks>, sender: Sender<Event<C>>) -> Self {
        Self {
            state: LifecycleObserverState::default(),
            platform_hooks,
            sender,
        }
    }

    pub fn take_user_stop_requested(&mut self) -> bool {
        let v = self.state.user_stop_requested;
        self.state.user_stop_requested = false;
        v
    }

    fn compute_last_shutdown(&self) -> CoreResult<i64> {
        let stored = self.state.last_process_stopped_shutdown;
        if stored > 0 {
            return Ok(stored);
        }
        Ok(self
            .platform_hooks
            .get_last_shutdown_time_utc_ms()?
            .unwrap_or(0))
    }

    fn on_event_while_running(&mut self, event: &Event<C>) -> CoreResult<()> {
        let now_ms = self.platform_hooks.get_time_utc_ms()?;
        let startup_time_ms = self.platform_hooks.get_last_startup_time_utc_ms()?;

        // Only pay the platform I/O cost for shutdown time when we might actually use it.
        let last_shutdown = if self.state.last_process_stopped_other > 0 {
            self.compute_last_shutdown()?
        } else {
            0
        };

        let old = self.state.clone();

        // Forward lifecycle events as batch log entries

        if last_shutdown > 0
            && self.state.last_process_stopped_other > 0
            && last_shutdown > self.state.last_process_stopped_other
            && self.state.last_process_stopped_shutdown < last_shutdown
        {
            // We missed the shutdown event, fill in with best effort
            self.sender
                .send(Event::Upload {
                    risk: 0.0,
                    kind: UploadKind::Lifecycle {
                        kind: LifecycleKind::ProcessStoppedShutdown,
                    },
                })
                .ok();
            self.state.last_process_stopped_shutdown = last_shutdown;
        }

        if let Some(boot_ms) = startup_time_ms
            && boot_ms > self.state.last_sent_boot
        {
            // New boot we haven't recorded yet, fill in with best effort
            self.sender
                .send(Event::Upload {
                    risk: 0.0,
                    kind: UploadKind::Lifecycle {
                        kind: LifecycleKind::ComputerBooted,
                    },
                })
                .ok();
            self.state.last_sent_boot = boot_ms;
        }

        let lifecycle_event: Option<(LifecycleKind, f32)> = match event {
            Event::ProcessStarted => Some((LifecycleKind::ProcessStarted, 0.0)),
            Event::ProcessStopped(reason) => match reason {
                ProcessStoppedReason::Other => Some((LifecycleKind::ProcessStoppedOther, 0.0)),
                ProcessStoppedReason::Shutdown => {
                    Some((LifecycleKind::ProcessStoppedShutdown, 0.0))
                }
                ProcessStoppedReason::User => Some((LifecycleKind::ProcessStoppedUser, 0.0)),
            },
            Event::ComputerSuspended => Some((LifecycleKind::ComputerSuspended, 0.0)),
            Event::UserSessionLogin => Some((LifecycleKind::Login, 0.0)),
            Event::UserSessionLogout => Some((LifecycleKind::Logout, HIGH_RISK_LIFECYCLE_ALERT)),
            _ => None,
        };
        if let Some((kind, risk)) = lifecycle_event {
            self.sender
                .send(Event::Upload {
                    risk,
                    kind: UploadKind::Lifecycle { kind },
                })
                .ok();
        }

        // Update state timestamps
        match event {
            Event::ProcessStopped(ProcessStoppedReason::Other) => {
                self.state.last_process_stopped_other = now_ms;
            }
            Event::ProcessStopped(ProcessStoppedReason::Shutdown) => {
                self.state.last_process_stopped_shutdown = now_ms;
            }
            Event::ProcessStopped(ProcessStoppedReason::User) => {
                self.state.last_process_stopped_user = now_ms;
            }
            Event::ProcessStarted => {
                self.state.last_process_started = now_ms;
                self.state.last_running_started = now_ms;
            }
            Event::ComputerSuspended => {
                self.state.last_computer_suspend = now_ms;
                self.state.status = LifecycleStatus::Suspended;
            }
            Event::ComputerResumed => {
                self.state.last_computer_resume = now_ms;
            }
            Event::Ping => {
                self.state.last_ping = now_ms;
            }
            Event::UserStopRequested { .. } => {
                self.state.user_stop_requested = true;
            }
            Event::UserSessionLogin | Event::Login { .. } => {
                self.state.last_login = now_ms;
            }
            _ => {}
        }

        // ALERT: process killed >10s before shutdown
        if matches!(event, Event::ProcessStarted)
            && old.last_process_stopped_other > 0
            && last_shutdown - old.last_process_stopped_other > 10000
        {
            self.sender
                .send(Event::Upload {
                    risk: 0.5,
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::ProcessKilledBeforeShutdown,
                    },
                })
                .ok();
        }

        // ALERT: user explicitly stopped the process
        if matches!(event, Event::ProcessStopped(ProcessStoppedReason::User)) {
            self.sender
                .send(Event::Upload {
                    risk: HIGH_RISK_LIFECYCLE_ALERT,
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::UserStoppedProcess,
                    },
                })
                .ok();
        }

        // ALERT: ProcessStarted after suspicious gap
        if matches!(event, Event::ProcessStarted)
            && now_ms - old.last_login > 60000
            && old.last_process_started > old.last_process_stopped_user
        {
            let boot_ms = startup_time_ms.unwrap_or(0);
            let ping_gap = if old.last_ping > 0 {
                now_ms - old.last_ping
            } else {
                i64::MAX
            };
            if ping_gap > 10000 && (now_ms - boot_ms) > 60000 {
                self.sender
                    .send(Event::Upload {
                        risk: HIGH_RISK_LIFECYCLE_ALERT,
                        kind: UploadKind::LifecycleAlert {
                            reason: AlertReason::UnexpectedProcessStart,
                        },
                    })
                    .ok();
            }
        }

        // ALERT: Ping gap while computer was running (not sleeping)
        if matches!(event, Event::Ping) && now_ms - old.last_login > 60000 {
            let ping_gap = now_ms - old.last_ping;
            let start_gap = now_ms - old.last_running_started;
            if old.last_ping > 0 && ping_gap > 10000 && start_gap > 10000 {
                self.sender
                    .send(Event::Upload {
                        risk: HIGH_RISK_LIFECYCLE_ALERT,
                        kind: UploadKind::LifecycleAlert {
                            reason: AlertReason::PingGapWhileRunning,
                        },
                    })
                    .ok();
            }
        }

        Ok(())
    }

    fn on_event_while_suspended(&mut self, event: &Event<C>) -> CoreResult<()> {
        let now_ms = self.platform_hooks.get_time_utc_ms()?;

        if matches!(event, Event::Ping) {
            self.state.pings_while_suspended += 1;
        }

        if matches!(event, Event::ComputerResumed) {
            self.state.pings_while_suspended = 0;
            self.state.status = LifecycleStatus::Running;
            self.state.last_running_started = now_ms;
            self.state.last_computer_resume = now_ms;
            self.sender
                .send(Event::Upload {
                    risk: 0.0,
                    kind: UploadKind::Lifecycle {
                        kind: LifecycleKind::ComputerResumed,
                    },
                })
                .ok();
        }

        // ProcessStopped and UserSessionLogout must be recorded even while suspended
        // so the server audit trail is correct and missed-shutdown detection stays accurate.
        match event {
            Event::ProcessStopped(reason) => {
                let kind = match reason {
                    ProcessStoppedReason::Other => LifecycleKind::ProcessStoppedOther,
                    ProcessStoppedReason::Shutdown => LifecycleKind::ProcessStoppedShutdown,
                    ProcessStoppedReason::User => LifecycleKind::ProcessStoppedUser,
                };
                self.sender
                    .send(Event::Upload {
                        risk: 0.0,
                        kind: UploadKind::Lifecycle { kind },
                    })
                    .ok();
                match reason {
                    ProcessStoppedReason::Other => self.state.last_process_stopped_other = now_ms,
                    ProcessStoppedReason::Shutdown => {
                        self.state.last_process_stopped_shutdown = now_ms
                    }
                    ProcessStoppedReason::User => self.state.last_process_stopped_user = now_ms,
                }
            }
            Event::UserSessionLogout => {
                self.sender
                    .send(Event::Upload {
                        risk: HIGH_RISK_LIFECYCLE_ALERT,
                        kind: UploadKind::Lifecycle {
                            kind: LifecycleKind::Logout,
                        },
                    })
                    .ok();
            }
            _ => {}
        }

        if self.state.pings_while_suspended > 3 {
            // Reset before sending so re-entrant processing of the queued Upload
            // event doesn't re-trigger this block while still in Suspended state.
            self.state.pings_while_suspended = 0;
            self.sender
                .send(Event::Upload {
                    risk: MEDIUM_RISK_LIFECYCLE_ALERT,
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::MissingResume,
                    },
                })
                .ok();
            self.sender.send(Event::ComputerResumed).ok();
        }

        Ok(())
    }
}

impl<C: 'static> Observer<C> for LifecycleObserver<C> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "lifecycle"
    }

    fn save_state(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.state)?)
    }

    fn load_state(&mut self, state: StateType) -> CoreResult<()> {
        self.state = serde_json::from_value(state)?;
        Ok(())
    }

    fn on_event(&mut self, event: &Event<C>) -> CoreResult<()> {
        if matches!(event, Event::StatusRequest) {
            // Responding at all means the monitor process is running.
            let last_loop_at_ms = (self.state.last_ping > 0).then_some(self.state.last_ping);
            self.sender
                .send(Event::PartialStatus(PartialStatus::Lifecycle {
                    is_running: true,
                    last_loop_at_ms,
                }))
                .ok();
            return Ok(());
        }
        match self.state.status {
            LifecycleStatus::Running => self.on_event_while_running(event),
            LifecycleStatus::Suspended => self.on_event_while_suspended(event),
        }
    }
}
