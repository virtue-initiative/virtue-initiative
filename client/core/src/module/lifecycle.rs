use std::sync::{Arc, Mutex};

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

pub struct LifecycleInner {
    pub state: LifecycleObserverState,
    platform: Box<dyn ScreenshotHooks>,
}

impl LifecycleInner {
    fn compute_last_shutdown(&self) -> CoreResult<i64> {
        let stored = self.state.last_process_stopped_shutdown;
        if stored > 0 {
            return Ok(stored);
        }
        Ok(self.platform.get_last_shutdown_time_utc_ms()?.unwrap_or(0))
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

        // Forward lifecycle event
        let _ = emitter.send(Upload {
            risk: 0.0,
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::ProcessStarted,
            },
        });

        self.state.last_process_started = now_ms;
        self.state.last_running_started = now_ms;

        // ALERT: process killed >10s before shutdown
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

        // ALERT: ProcessStarted after suspicious gap
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

    fn handle_process_stopped_running(
        &mut self,
        reason: &ProcessStoppedReason,
        emitter: &Emitter,
    ) -> CoreResult<()> {
        let now_ms = self.platform.get_time_utc_ms()?;
        let startup_time_ms = self.platform.get_last_startup_time_utc_ms()?;
        let last_shutdown = if self.state.last_process_stopped_other > 0 {
            self.compute_last_shutdown()?
        } else {
            0
        };
        self.maybe_backfill_missed_events(last_shutdown, startup_time_ms, emitter)?;

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
                let _ = emitter.send(Upload {
                    risk: HIGH_RISK_LIFECYCLE_ALERT,
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::UserStoppedProcess,
                    },
                });
            }
        }

        Ok(())
    }

    fn handle_process_stopped_suspended(
        &mut self,
        reason: &ProcessStoppedReason,
        emitter: &Emitter,
    ) -> CoreResult<()> {
        let now_ms = self.platform.get_time_utc_ms()?;
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
            ProcessStoppedReason::User => self.state.last_process_stopped_user = now_ms,
        }
        Ok(())
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

pub struct LifecycleModule {
    pub inner: Arc<Mutex<LifecycleInner>>,
}

impl LifecycleModule {
    pub fn new(platform: Box<dyn ScreenshotHooks>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(LifecycleInner {
                state: LifecycleObserverState::default(),
                platform,
            })),
        }
    }
}

impl Observer for LifecycleModule {
    fn name(&self) -> &'static str {
        "lifecycle"
    }

    fn init(&mut self, bus: &mut EventBus, state: StateType) -> CoreResult<()> {
        if !state.is_null() {
            self.inner.lock().unwrap().state = serde_json::from_value(state)?;
        }

        let emitter = bus.emitter();

        // StatusRequest: always respond
        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |_: &StatusRequest| {
            let g = inner.lock().unwrap();
            let last_loop_at_ms = (g.state.last_ping > 0).then_some(g.state.last_ping);
            let _ = e.send(PartialStatus::Lifecycle {
                is_running: true,
                last_loop_at_ms,
            });
            Ok(())
        });

        // ProcessStarted: only while Running
        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |_: &ProcessStarted| {
            let mut g = inner.lock().unwrap();
            if matches!(g.state.status, LifecycleStatus::Running) {
                g.handle_process_started(&e)?;
            }
            Ok(())
        });

        // ProcessStopped: both states
        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |ev: &ProcessStopped| {
            let mut g = inner.lock().unwrap();
            match g.state.status.clone() {
                LifecycleStatus::Running => g.handle_process_stopped_running(&ev.0, &e)?,
                LifecycleStatus::Suspended => g.handle_process_stopped_suspended(&ev.0, &e)?,
            }
            Ok(())
        });

        // ComputerSuspended: only while Running → transitions to Suspended
        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |_: &ComputerSuspended| {
            let mut g = inner.lock().unwrap();
            if matches!(g.state.status, LifecycleStatus::Running) {
                let now_ms = g.platform.get_time_utc_ms()?;
                let _ = e.send(Upload {
                    risk: 0.0,
                    kind: UploadKind::Lifecycle {
                        kind: LifecycleKind::ComputerSuspended,
                    },
                });
                g.state.last_computer_suspend = now_ms;
                g.state.status = LifecycleStatus::Suspended;
            }
            Ok(())
        });

        // ComputerResumed: only while Suspended → transitions to Running
        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |_: &ComputerResumed| {
            let mut g = inner.lock().unwrap();
            if matches!(g.state.status, LifecycleStatus::Suspended) {
                let now_ms = g.platform.get_time_utc_ms()?;
                g.state.pings_while_suspended = 0;
                g.state.status = LifecycleStatus::Running;
                g.state.last_running_started = now_ms;
                g.state.last_computer_resume = now_ms;
                let _ = e.send(Upload {
                    risk: 0.0,
                    kind: UploadKind::Lifecycle {
                        kind: LifecycleKind::ComputerResumed,
                    },
                });
            }
            Ok(())
        });

        // UserSessionLogin: only while Running
        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |_: &UserSessionLogin| {
            let mut g = inner.lock().unwrap();
            if matches!(g.state.status, LifecycleStatus::Running) {
                let now_ms = g.platform.get_time_utc_ms()?;
                let _ = e.send(Upload {
                    risk: 0.0,
                    kind: UploadKind::Lifecycle {
                        kind: LifecycleKind::Login,
                    },
                });
                g.state.last_login = now_ms;
            }
            Ok(())
        });

        // UserSessionLogout: both states
        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |_: &UserSessionLogout| {
            let g = inner.lock().unwrap();
            let _ = e.send(Upload {
                risk: HIGH_RISK_LIFECYCLE_ALERT,
                kind: UploadKind::Lifecycle {
                    kind: LifecycleKind::Logout,
                },
            });
            drop(g);
            Ok(())
        });

        // Ping: different logic per state
        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |_: &Ping| {
            let mut g = inner.lock().unwrap();
            match g.state.status.clone() {
                LifecycleStatus::Running => g.handle_ping_running(&e)?,
                LifecycleStatus::Suspended => g.handle_ping_suspended(&e)?,
            }
            Ok(())
        });

        // Login (auth event): update last_login timestamp while Running
        let inner = Arc::clone(&self.inner);
        bus.subscribe(move |_: &Login| {
            let mut g = inner.lock().unwrap();
            if matches!(g.state.status, LifecycleStatus::Running) {
                let now_ms = g.platform.get_time_utc_ms()?;
                g.state.last_login = now_ms;
            }
            Ok(())
        });

        // UserStopRequested: record intent
        let inner = Arc::clone(&self.inner);
        bus.subscribe(move |_: &UserStopRequested| {
            inner.lock().unwrap().state.user_stop_requested = true;
            Ok(())
        });

        Ok(())
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.inner.lock().unwrap().state)?)
    }
}
