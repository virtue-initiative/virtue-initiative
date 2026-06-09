use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};
use crate::events::types::{
    AlertReason, ComputerResumed, ComputerSuspended, Login, PartialStatus, Ping, ProcessStarted,
    ProcessStopped, StatusRequest, Upload, UserSessionLogin, UserSessionLogout, UserStopRequested,
};
use crate::model::{LifecycleKind, ProcessStoppedReason, UploadKind};
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
#[serde(default)]
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

pub struct LifecycleModule {
    pub state: LifecycleObserverState,
    platform: Box<dyn ScreenshotHooks>,
}

impl LifecycleModule {
    pub fn new(platform: Box<dyn ScreenshotHooks>) -> Self {
        Self {
            state: LifecycleObserverState::default(),
            platform,
        }
    }

    fn compute_last_shutdown(&self) -> CoreResult<i64> {
        let stored = self.state.last_process_stopped_shutdown;
        if stored > 0 {
            return Ok(stored);
        }
        Ok(self.platform.get_last_shutdown_time_utc_ms()?.unwrap_or(0))
    }

    fn maybe_backfill_missed_events(
        &mut self,
        last_shutdown: i64,
        startup_time_ms: Option<i64>,
        emitter: &Emitter,
    ) -> CoreResult<()> {
        if last_shutdown > 0
            && self.state.last_process_stopped_other > 0
            && last_shutdown > self.state.last_process_stopped_other
            && self.state.last_process_stopped_shutdown < last_shutdown
        {
            let _ = emitter.send(Upload {
                risk: 0.0,
                kind: UploadKind::Lifecycle {
                    kind: LifecycleKind::ProcessStoppedShutdown,
                },
            });
            self.state.last_process_stopped_shutdown = last_shutdown;
        }

        if let Some(boot_ms) = startup_time_ms {
            if boot_ms > self.state.last_sent_boot {
                let _ = emitter.send(Upload {
                    risk: 0.0,
                    kind: UploadKind::Lifecycle {
                        kind: LifecycleKind::ComputerBooted,
                    },
                });
                self.state.last_sent_boot = boot_ms;
            }
        }

        Ok(())
    }

    fn handle_process_started(&mut self, emitter: &Emitter) -> CoreResult<()> {
        let now_ms = self.platform.get_time_utc_ms()?;
        let startup_time_ms = self.platform.get_last_startup_time_utc_ms()?;
        let last_shutdown = if self.state.last_process_stopped_other > 0 {
            self.compute_last_shutdown()?
        } else {
            0
        };
        let old = self.state.clone();

        self.maybe_backfill_missed_events(last_shutdown, startup_time_ms, emitter)?;

        let _ = emitter.send(Upload {
            risk: 0.0,
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::ProcessStarted,
            },
        });

        self.state.last_process_started = now_ms;
        self.state.last_running_started = now_ms;

        if old.last_process_stopped_other > 0
            && last_shutdown - old.last_process_stopped_other > 10000
        {
            let _ = emitter.send(Upload {
                risk: 0.5,
                kind: UploadKind::LifecycleAlert {
                    reason: AlertReason::ProcessKilledBeforeShutdown,
                },
            });
        }

        if now_ms - old.last_login > 60000
            && old.last_process_started > old.last_process_stopped_user
        {
            let boot_ms = startup_time_ms.unwrap_or(0);
            let ping_gap = if old.last_ping > 0 {
                now_ms - old.last_ping
            } else {
                i64::MAX
            };
            if ping_gap > 10000 && (now_ms - boot_ms) > 60000 {
                let _ = emitter.send(Upload {
                    risk: HIGH_RISK_LIFECYCLE_ALERT,
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::UnexpectedProcessStart,
                    },
                });
            }
        }

        Ok(())
    }

    fn handle_process_stopped(
        &mut self,
        reason: &ProcessStoppedReason,
        emitter: &Emitter,
    ) -> CoreResult<()> {
        let now_ms = self.platform.get_time_utc_ms()?;
        match self.state.status.clone() {
            LifecycleStatus::Running => {
                let startup_time_ms = self.platform.get_last_startup_time_utc_ms()?;
                let last_shutdown = if self.state.last_process_stopped_other > 0 {
                    self.compute_last_shutdown()?
                } else {
                    0
                };
                self.maybe_backfill_missed_events(last_shutdown, startup_time_ms, emitter)?;
            }
            LifecycleStatus::Suspended => {}
        }

        let kind = match reason {
            ProcessStoppedReason::Other => LifecycleKind::ProcessStoppedOther,
            ProcessStoppedReason::Shutdown => LifecycleKind::ProcessStoppedShutdown,
            ProcessStoppedReason::User => LifecycleKind::ProcessStoppedUser,
        };
        let _ = emitter.send(Upload {
            risk: 0.0,
            kind: UploadKind::Lifecycle { kind },
        });

        match reason {
            ProcessStoppedReason::Other => self.state.last_process_stopped_other = now_ms,
            ProcessStoppedReason::Shutdown => self.state.last_process_stopped_shutdown = now_ms,
            ProcessStoppedReason::User => {
                self.state.last_process_stopped_user = now_ms;
                if matches!(self.state.status, LifecycleStatus::Running) {
                    let _ = emitter.send(Upload {
                        risk: HIGH_RISK_LIFECYCLE_ALERT,
                        kind: UploadKind::LifecycleAlert {
                            reason: AlertReason::UserStoppedProcess,
                        },
                    });
                }
            }
        }

        Ok(())
    }

    fn handle_ping(&mut self, emitter: &Emitter) -> CoreResult<()> {
        match self.state.status.clone() {
            LifecycleStatus::Running => self.handle_ping_running(emitter),
            LifecycleStatus::Suspended => self.handle_ping_suspended(emitter),
        }
    }

    fn handle_ping_running(&mut self, emitter: &Emitter) -> CoreResult<()> {
        let now_ms = self.platform.get_time_utc_ms()?;
        let startup_time_ms = self.platform.get_last_startup_time_utc_ms()?;
        let last_shutdown = if self.state.last_process_stopped_other > 0 {
            self.compute_last_shutdown()?
        } else {
            0
        };
        let old = self.state.clone();
        self.maybe_backfill_missed_events(last_shutdown, startup_time_ms, emitter)?;
        self.state.last_ping = now_ms;

        if now_ms - old.last_login > 60000 {
            let ping_gap = now_ms - old.last_ping;
            let start_gap = now_ms - old.last_running_started;
            if old.last_ping > 0 && ping_gap > 10000 && start_gap > 10000 {
                let _ = emitter.send(Upload {
                    risk: HIGH_RISK_LIFECYCLE_ALERT,
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::PingGapWhileRunning,
                    },
                });
            }
        }

        Ok(())
    }

    fn handle_ping_suspended(&mut self, emitter: &Emitter) -> CoreResult<()> {
        self.state.pings_while_suspended += 1;
        if self.state.pings_while_suspended > 3 {
            self.state.pings_while_suspended = 0;
            let _ = emitter.send(Upload {
                risk: MEDIUM_RISK_LIFECYCLE_ALERT,
                kind: UploadKind::LifecycleAlert {
                    reason: AlertReason::MissingResume,
                },
            });
            let _ = emitter.send(ComputerResumed);
        }
        Ok(())
    }
}

impl Observer for LifecycleModule {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "lifecycle"
    }

    fn init(&mut self, _bus: &mut EventBus, state: StateType) -> CoreResult<()> {
        if !state.is_null() {
            self.state = serde_json::from_value(state)?;
        }
        Ok(())
    }

    fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
        crate::dispatch_event!(event, {
            _: Ping => self.handle_ping(emitter),
            _: Login => {
                if matches!(self.state.status, LifecycleStatus::Running) {
                    self.state.last_login = self.platform.get_time_utc_ms()?;
                }
                Ok(())
            },
            _: ProcessStarted => {
                if matches!(self.state.status, LifecycleStatus::Running) {
                    self.handle_process_started(emitter)
                } else {
                    Ok(())
                }
            },
            ev: ProcessStopped => self.handle_process_stopped(&ev.0, emitter),
            _: ComputerSuspended => {
                if matches!(self.state.status, LifecycleStatus::Running) {
                    let now_ms = self.platform.get_time_utc_ms()?;
                    let _ = emitter.send(Upload {
                        risk: 0.0,
                        kind: UploadKind::Lifecycle { kind: LifecycleKind::ComputerSuspended },
                    });
                    self.state.last_computer_suspend = now_ms;
                    self.state.status = LifecycleStatus::Suspended;
                }
                Ok(())
            },
            _: ComputerResumed => {
                if matches!(self.state.status, LifecycleStatus::Suspended) {
                    let now_ms = self.platform.get_time_utc_ms()?;
                    self.state.pings_while_suspended = 0;
                    self.state.status = LifecycleStatus::Running;
                    self.state.last_running_started = now_ms;
                    self.state.last_computer_resume = now_ms;
                    let _ = emitter.send(Upload {
                        risk: 0.0,
                        kind: UploadKind::Lifecycle { kind: LifecycleKind::ComputerResumed },
                    });
                }
                Ok(())
            },
            _: UserSessionLogin => {
                if matches!(self.state.status, LifecycleStatus::Running) {
                    let now_ms = self.platform.get_time_utc_ms()?;
                    let _ = emitter.send(Upload {
                        risk: 0.0,
                        kind: UploadKind::Lifecycle { kind: LifecycleKind::Login },
                    });
                    self.state.last_login = now_ms;
                }
                Ok(())
            },
            _: UserSessionLogout => {
                let _ = emitter.send(Upload {
                    risk: HIGH_RISK_LIFECYCLE_ALERT,
                    kind: UploadKind::Lifecycle { kind: LifecycleKind::Logout },
                });
                Ok(())
            },
            _: StatusRequest => {
                let last_loop_at_ms = (self.state.last_ping > 0).then_some(self.state.last_ping);
                let _ = emitter.send(PartialStatus::Lifecycle { is_running: true, last_loop_at_ms });
                Ok(())
            },
            _: UserStopRequested => {
                self.state.user_stop_requested = true;
                Ok(())
            },
        })
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.state)?)
    }
}
