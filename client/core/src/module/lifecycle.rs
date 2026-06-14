use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::Ping;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};
use crate::model::{AlertReason, PartialStatus};
use crate::model::{LifecycleKind, ProcessStoppedReason, UploadKind};
use crate::module::auth::Login;
use crate::module::screenshot::{ScreenshotPaused, ScreenshotResumed};
use crate::module::status::StatusRequest;
use crate::module::upload::Upload;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessStarted;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessStopped(pub ProcessStoppedReason);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerSuspended;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerResumed;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSessionLogin;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSessionLogout;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserStopRequested {
    pub source: String,
}
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

        if let Some(boot_ms) = startup_time_ms
            && boot_ms > self.state.last_sent_boot
        {
            let _ = emitter.send(Upload {
                risk: 0.0,
                kind: UploadKind::Lifecycle {
                    kind: LifecycleKind::ComputerBooted,
                },
            });
            self.state.last_sent_boot = boot_ms;
        }

        Ok(())
    }

    fn handle_process_started(&mut self, emitter: &Emitter) -> CoreResult<()> {
        let now_ms = self.platform.get_time_utc_ms()?;
        let startup_time_ms = self.platform.get_last_startup_time_utc_ms()?;
        let last_shutdown = if self.state.last_process_stopped_other > 0 || self.state.last_ping > 0
        {
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

        // Detect force kill: process was actively pinging but disappeared without any
        // ProcessStopped event, and the computer then shut down 30+ s after the last ping.
        if last_shutdown > 0
            && old.last_ping > 0
            && old.last_ping >= old.last_process_started
            && old.last_ping > old.last_process_stopped_other
            && old.last_ping > old.last_process_stopped_user
            && last_shutdown - old.last_ping > 30_000
        {
            let _ = emitter.send(Upload {
                risk: HIGH_RISK_LIFECYCLE_ALERT,
                kind: UploadKind::LifecycleAlert {
                    reason: AlertReason::ForceKilledBeforeShutdown,
                },
            });
        }

        if now_ms - old.last_login > 120000
            && old.last_process_started > old.last_process_stopped_user
        {
            let boot_ms = startup_time_ms.unwrap_or(0);
            let ping_gap = if old.last_ping > 0 {
                now_ms - old.last_ping
            } else {
                i64::MAX
            };
            if ping_gap > 10000 && (now_ms - boot_ms) > 120000 {
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

        if now_ms - old.last_login > 120000 {
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
                    let _ = emitter.send(ScreenshotResumed {});
                    self.state.last_login = now_ms;
                }
                Ok(())
            },
            _: UserSessionLogout => {
                let _ = emitter.send(Upload {
                    risk: HIGH_RISK_LIFECYCLE_ALERT,
                    kind: UploadKind::Lifecycle { kind: LifecycleKind::Logout },
                });
                let _ = emitter.send(ScreenshotPaused {});
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

#[cfg(test)]
mod tests {
    use super::{
        ComputerResumed, ComputerSuspended, ProcessStarted, ProcessStopped, UserSessionLogin,
        UserSessionLogout,
    };
    use super::{LifecycleModule, LifecycleStatus};
    use crate::events::Ping;
    use crate::model::PartialStatus;
    use crate::model::{AlertReason, LifecycleKind, ProcessStoppedReason, UploadKind};
    use crate::module::status::StatusRequest;
    use crate::module::upload::Upload;
    use crate::testing::EventTester;

    #[test]
    fn status_request_emits_lifecycle_partial_status() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(1, StatusRequest);
        t.assert_like::<PartialStatus>(crate::like!(PartialStatus::Lifecycle { .. }));
    }

    #[test]
    fn process_started_emits_lifecycle_upload() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(1, ProcessStarted);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::ProcessStarted
            },
            ..
        }));
        assert_eq!(
            t.observer::<LifecycleModule>().state.last_process_started,
            1_000
        );
        assert_eq!(
            t.observer::<LifecycleModule>().state.last_running_started,
            1_000
        );
    }

    #[test]
    fn process_stopped_shutdown_emits_upload() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(2, ProcessStopped(ProcessStoppedReason::Shutdown));
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::ProcessStoppedShutdown
            },
            ..
        }));
        assert_eq!(
            t.observer::<LifecycleModule>()
                .state
                .last_process_stopped_shutdown,
            2_000
        );
    }

    #[test]
    fn process_stopped_user_emits_upload_and_high_risk_alert() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(4, ProcessStopped(ProcessStoppedReason::User));
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::ProcessStoppedUser
            },
            ..
        }));
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UserStoppedProcess
            },
            ..
        }));
        let alert = t
            .captured::<Upload>()
            .into_iter()
            .find(|e| {
                matches!(
                    e.kind,
                    UploadKind::LifecycleAlert {
                        reason: AlertReason::UserStoppedProcess
                    }
                )
            })
            .unwrap();
        assert!(alert.risk >= 0.9, "alert should be high risk");
        assert_eq!(
            t.observer::<LifecycleModule>()
                .state
                .last_process_stopped_user,
            4_000
        );
    }

    #[test]
    fn computer_suspended_emits_upload() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(5, ComputerSuspended);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::ComputerSuspended
            },
            ..
        }));
        assert!(matches!(
            t.observer::<LifecycleModule>().state.status,
            LifecycleStatus::Suspended
        ));
        assert_eq!(
            t.observer::<LifecycleModule>().state.last_computer_suspend,
            5_000
        );
    }

    #[test]
    fn computer_resumed_after_suspend_emits_upload() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(6, ComputerSuspended);
        t.clear_captured();
        t.emit(7, ComputerResumed);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::ComputerResumed
            },
            ..
        }));
        assert!(matches!(
            t.observer::<LifecycleModule>().state.status,
            LifecycleStatus::Running
        ));
        assert_eq!(
            t.observer::<LifecycleModule>().state.last_computer_resume,
            7_000
        );
        assert_eq!(
            t.observer::<LifecycleModule>().state.last_running_started,
            7_000
        );
    }

    #[test]
    fn fourth_ping_while_suspended_triggers_missing_resume_alert() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();

        t.emit(1, ComputerSuspended);
        t.clear_captured();

        // Pings at 2s, 3s, 4s (3 pings) — NOT at 5s (exclusive boundary).
        t.enable_pings(2);
        t.disable_pings(5);
        t.advance_to(5);
        assert_eq!(
            t.observer::<LifecycleModule>().state.pings_while_suspended,
            3,
            "counter should be 3 before the 4th ping"
        );
        t.clear_captured();

        // 4th ping crosses >3 threshold → MissingResume + auto ComputerResumed
        t.emit(6, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::MissingResume
            },
            ..
        }));
        assert_eq!(
            t.observer::<LifecycleModule>().state.pings_while_suspended,
            0,
            "counter should reset after alert"
        );
        assert!(
            matches!(
                t.observer::<LifecycleModule>().state.status,
                LifecycleStatus::Running
            ),
            "auto-sent ComputerResumed should restore Running status"
        );
    }

    #[test]
    fn session_login_emits_lifecycle_upload() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(1, UserSessionLogin);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::Login
            },
            ..
        }));
        assert_eq!(t.observer::<LifecycleModule>().state.last_login, 1_000);
    }

    #[test]
    fn session_logout_emits_high_risk_lifecycle_upload() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(1, UserSessionLogout);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::Logout
            },
            ..
        }));
        let upload = t
            .captured::<Upload>()
            .into_iter()
            .find(|e| {
                matches!(
                    e.kind,
                    UploadKind::Lifecycle {
                        kind: LifecycleKind::Logout
                    }
                )
            })
            .unwrap();
        assert!(upload.risk >= 0.9, "logout upload should be high risk");
    }

    #[test]
    fn ping_gap_while_running_emits_high_risk_alert() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();

        // ProcessStarted + first Ping both at 1s — within 120s grace window, no alert.
        t.emit(1, ProcessStarted);
        t.emit(1, Ping);
        assert_eq!(t.observer::<LifecycleModule>().state.last_ping, 1_000);
        t.clear_captured();

        // Jump to 200s: past 120s grace window and 10s ping-gap threshold.
        t.emit(200, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::PingGapWhileRunning
            },
            ..
        }));
        assert_eq!(t.observer::<LifecycleModule>().state.last_ping, 200_000);
        let alert = t
            .captured::<Upload>()
            .into_iter()
            .find(|e| {
                matches!(
                    e.kind,
                    UploadKind::LifecycleAlert {
                        reason: AlertReason::PingGapWhileRunning
                    }
                )
            })
            .unwrap();
        assert!(alert.risk >= 0.9, "ping gap alert should be high risk");
    }

    #[test]
    fn ping_within_login_grace_period_does_not_emit_alert() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(1, ProcessStarted);
        t.emit(1, Ping);
        t.clear_captured();
        t.emit(20, UserSessionLogin);
        assert_eq!(t.observer::<LifecycleModule>().state.last_login, 20_000);
        t.emit(30, Ping);
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::PingGapWhileRunning
            },
            ..
        }));
    }

    #[test]
    fn process_killed_before_shutdown_emits_alert() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(1, ProcessStarted);
        t.emit(1, ProcessStopped(ProcessStoppedReason::Other));
        assert_eq!(
            t.observer::<LifecycleModule>()
                .state
                .last_process_stopped_other,
            1_000
        );
        t.emit(12, ProcessStopped(ProcessStoppedReason::Shutdown));
        assert_eq!(
            t.observer::<LifecycleModule>()
                .state
                .last_process_stopped_shutdown,
            12_000
        );
        t.clear_captured();
        // Gap (12_000 - 1_000 = 11_000 ms) exceeds the 10 s threshold
        t.emit(20, ProcessStarted);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::ProcessKilledBeforeShutdown
            },
            ..
        }));
    }

    #[test]
    fn force_killed_process_with_platform_shutdown_emits_high_risk_alert() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();

        // Run 1: process starts, pings, then is killed (Other stop — no shutdown event yet)
        t.emit(1, ProcessStarted);
        t.emit(2, Ping);
        t.emit(3, Ping);
        t.emit(10, ProcessStopped(ProcessStoppedReason::Other));
        assert_eq!(
            t.observer::<LifecycleModule>()
                .state
                .last_process_stopped_other,
            10_000
        );

        // Run 2: process starts, pings, then is force-killed (no ProcessStopped event)
        t.emit(12, ProcessStarted);
        t.emit(13, Ping);
        t.emit(14, Ping);
        t.emit(15, Ping);
        assert_eq!(t.observer::<LifecycleModule>().state.last_ping, 15_000);
        assert_eq!(
            t.observer::<LifecycleModule>()
                .state
                .last_process_stopped_shutdown,
            0,
            "no shutdown recorded yet"
        );

        // Computer shuts down at 70s (55 s after last ping), boots at 100s.
        // Only the platform hook carries this — no events were sent.
        // Platform hooks are now implemented on Linux/Mac; this mock mirrors a real capability.
        t.platform.set_last_shutdown_time(Some(70_000));
        t.platform.set_last_startup_time(Some(100_000));
        t.clear_captured();

        // Run 3: lifecycle detects the force-kill gap (boot was only 10 s ago → no UnexpectedStart)
        t.emit(110, ProcessStarted);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::ForceKilledBeforeShutdown
            },
            ..
        }));
        let alert = t
            .captured::<Upload>()
            .into_iter()
            .find(|e| {
                matches!(
                    e.kind,
                    UploadKind::LifecycleAlert {
                        reason: AlertReason::ForceKilledBeforeShutdown
                    }
                )
            })
            .unwrap();
        assert!(alert.risk >= 0.9, "alert should be high risk");
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::ProcessStoppedShutdown
            },
            ..
        }));
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UnexpectedProcessStart
            },
            ..
        }));
        assert_eq!(
            t.observer::<LifecycleModule>()
                .state
                .last_process_stopped_shutdown,
            70_000,
            "shutdown time should be backfilled into state"
        );
    }

    #[test]
    fn state_round_trips_through_save_and_load() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();

        t.emit(30, UserSessionLogin); // last_login = 30_000
        t.emit(55, ProcessStarted); // last_running_started = 55_000
        t.emit(99, Ping); // last_ping = 99_000 (first ping — no gap alert)
        t.emit(99, ComputerSuspended); // status = Suspended
        t.emit(99, Ping); // pings_while_suspended = 1
        t.emit(99, Ping); // pings_while_suspended = 2

        {
            let s = &t.observer::<LifecycleModule>().state;
            assert_eq!(s.last_login, 30_000);
            assert_eq!(s.last_running_started, 55_000);
            assert_eq!(s.last_ping, 99_000);
            assert_eq!(s.pings_while_suspended, 2);
            assert!(matches!(s.status, LifecycleStatus::Suspended));
        }

        let saved = t.bus.save().unwrap();

        let mut b2 = EventTester::builder();
        b2.add(LifecycleModule::new(Box::new(b2.platform())));
        b2.with_state(saved);
        let mut t2 = b2.build();

        let s2 = &t2.observer::<LifecycleModule>().state;
        assert_eq!(s2.last_login, 30_000);
        assert_eq!(s2.last_running_started, 55_000);
        assert_eq!(s2.last_ping, 99_000);
        assert_eq!(s2.pings_while_suspended, 2);
        assert!(matches!(s2.status, LifecycleStatus::Suspended));
    }
}
