use std::any::Any;
use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::{
    AlertReason, Event, LifecycleKind, Observer, ProcessStoppedReason, StateType, UploadKind,
};
use crate::model::UserSessionState;
use crate::platform::PlatformHooks;

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

    // Running
    pub last_process_stopped_other: i64,
    pub last_process_stopped_shutdown: i64,
    pub last_process_stopped_user: i64,
    pub last_computer_suspend: i64,
    pub last_computer_resume: i64,
    pub last_ping: i64,
    pub last_process_started: i64,
    pub last_running_started: i64,

    // Suspended
    pub pings_while_suspended: i64,
}

pub struct LifecycleObserver {
    pub state: LifecycleObserverState,
    platform_hooks: Box<dyn PlatformHooks>,
    sender: Sender<Event>,
}

impl LifecycleObserver {
    pub fn new(platform_hooks: Box<dyn PlatformHooks>, sender: Sender<Event>) -> Self {
        Self {
            state: LifecycleObserverState::default(),
            platform_hooks,
            sender,
        }
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

    fn on_event_while_running(&mut self, event: &Event) -> CoreResult<()> {
        let now_ms = self.platform_hooks.get_time_utc_ms()?;
        let last_shutdown = self.compute_last_shutdown()?;
        let old_last_ping = self.state.last_ping;

        // ALERT: process killed >10s before shutdown
        if self.state.last_process_stopped_other > self.state.last_process_started
            && last_shutdown - self.state.last_process_stopped_other > 10000
        {
            self.sender.send(Event::Upload {
                risk: 0.5,
                kind: UploadKind::LifecycleAlert {
                    reason: AlertReason::ProcessKilledBeforeShutdown,
                },
            })?;
            self.state.last_process_stopped_other = 0;
        }

        // Forward lifecycle events as batch log entries
        let lifecycle_event: Option<(LifecycleKind, Option<UserSessionState>)> = match event {
            Event::ProcessStarted => Some((LifecycleKind::ProcessStarted, None)),
            Event::ProcessStopped(reason) => match reason {
                ProcessStoppedReason::Other => Some((LifecycleKind::ProcessStoppedOther, None)),
                ProcessStoppedReason::Shutdown => {
                    Some((LifecycleKind::ProcessStoppedShutdown, None))
                }
                ProcessStoppedReason::User => Some((LifecycleKind::ProcessStoppedUser, None)),
            },
            Event::ComputerSuspended => Some((LifecycleKind::ComputerSuspended, None)),
            Event::UserSessionChanged(state) => {
                Some((LifecycleKind::UserSessionChanged, Some(*state)))
            }
            _ => None,
        };
        if let Some((kind, session_state)) = lifecycle_event {
            self.sender.send(Event::Upload {
                risk: 0.0,
                kind: UploadKind::Lifecycle {
                    kind,
                    session_state,
                },
            })?;
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
            _ => {}
        }

        // ALERT: user explicitly stopped the process
        if matches!(event, Event::ProcessStopped(ProcessStoppedReason::User)) {
            self.sender.send(Event::Upload {
                risk: HIGH_RISK_LIFECYCLE_ALERT,
                kind: UploadKind::LifecycleAlert {
                    reason: AlertReason::UserStoppedProcess,
                },
            })?;
        }

        // ALERT: ProcessStarted after suspicious gap (state.last_ping is old value here)
        if matches!(event, Event::ProcessStarted) {
            let boot_ms = self
                .platform_hooks
                .get_last_startup_time_utc_ms()?
                .unwrap_or(0);
            let ping_gap = if old_last_ping > 0 {
                now_ms - old_last_ping
            } else {
                i64::MAX
            };
            if ping_gap > 10000 && (now_ms - boot_ms) > 10000 {
                self.sender.send(Event::Upload {
                    risk: HIGH_RISK_LIFECYCLE_ALERT,
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::UnexpectedProcessStart,
                    },
                })?;
            }
        }

        // ALERT: Ping gap while computer was running (not sleeping)
        if matches!(event, Event::Ping) {
            let ping_gap = now_ms - old_last_ping;
            let start_gap = now_ms - self.state.last_running_started;
            if old_last_ping > 0 && ping_gap > 10000 && start_gap > 10000 {
                self.sender.send(Event::Upload {
                    risk: HIGH_RISK_LIFECYCLE_ALERT,
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::PingGapWhileRunning,
                    },
                })?;
            }
        }

        Ok(())
    }

    fn on_event_while_suspended(&mut self, event: &Event) -> CoreResult<()> {
        let now_ms = self.platform_hooks.get_time_utc_ms()?;

        if matches!(event, Event::Ping) {
            self.state.pings_while_suspended += 1;
        }

        if matches!(event, Event::ComputerResumed) {
            self.state.pings_while_suspended = 0;
            self.state.status = LifecycleStatus::Running;
            self.state.last_running_started = now_ms;
            self.sender.send(Event::Upload {
                risk: 0.0,
                kind: UploadKind::Lifecycle {
                    kind: LifecycleKind::ComputerResumed,
                    session_state: None,
                },
            })?;
        }

        if self.state.pings_while_suspended > 3 {
            // We somehow missed the resume event
            self.sender.send(Event::Upload {
                risk: MEDIUM_RISK_LIFECYCLE_ALERT,
                kind: UploadKind::LifecycleAlert {
                    reason: AlertReason::MissingResume,
                },
            })?;

            // Fill in missing resume event
            self.sender.send(Event::ComputerResumed)?;
        }

        Ok(())
    }
}

impl Observer for LifecycleObserver {
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

    fn on_event(&mut self, event: &Event) -> CoreResult<()> {
        match self.state.status {
            LifecycleStatus::Running => self.on_event_while_running(event),
            LifecycleStatus::Suspended => self.on_event_while_suspended(event),
        }
    }
}
