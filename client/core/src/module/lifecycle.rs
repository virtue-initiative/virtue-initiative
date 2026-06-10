use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::Ping;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};
use crate::model::{AlertReason, PartialStatus};
use crate::model::{LifecycleKind, ProcessStoppedReason, UploadKind};
use crate::module::auth::Login;
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{
        ComputerResumed, ComputerSuspended, ProcessStarted, ProcessStopped, UserSessionLogin,
        UserSessionLogout,
    };
    use super::{LifecycleModule, LifecycleObserverState, LifecycleStatus};
    use crate::events::Ping;
    use crate::events::bus::{EventBus, StateType};
    use crate::model::PartialStatus;
    use crate::model::{AlertReason, LifecycleKind, ProcessStoppedReason, UploadKind};
    use crate::module::status::StatusRequest;
    use crate::module::upload::Upload;
    use crate::testing::{MockClock, TestPlatformHooks};

    type BusWithCapture = (
        EventBus,
        MockClock,
        Arc<Mutex<Vec<Upload>>>,
        Arc<Mutex<Vec<PartialStatus>>>,
    );

    fn make(ts: i64) -> BusWithCapture {
        let platform = TestPlatformHooks::new();
        platform.clock.set(ts);
        let clock = platform.clock.clone();
        let module = LifecycleModule::new(Box::new(platform));
        let uploads: Arc<Mutex<Vec<Upload>>> = Arc::new(Mutex::new(Vec::new()));
        let partials: Arc<Mutex<Vec<PartialStatus>>> = Arc::new(Mutex::new(Vec::new()));
        let u = Arc::clone(&uploads);
        let p = Arc::clone(&partials);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &Upload| {
            u.lock().unwrap().push(ev.clone());
            Ok(())
        });
        bus.subscribe(move |ev: &PartialStatus| {
            p.lock().unwrap().push(ev.clone());
            Ok(())
        });
        (bus, clock, uploads, partials)
    }

    fn get_state(bus: &mut EventBus) -> LifecycleObserverState {
        bus.observer_mut("lifecycle")
            .unwrap()
            .as_any_mut()
            .downcast_mut::<LifecycleModule>()
            .unwrap()
            .state
            .clone()
    }

    #[test]
    fn status_request_emits_lifecycle_partial_status() {
        let (mut bus, _clock, _, partials) = make(1_000);
        bus.send(StatusRequest).unwrap();
        bus.iter().unwrap();
        let p = partials.lock().unwrap();
        assert!(
            p.iter()
                .any(|s| matches!(s, PartialStatus::Lifecycle { .. })),
            "expected PartialStatus::Lifecycle"
        );
    }

    #[test]
    fn process_started_emits_lifecycle_upload() {
        let (mut bus, _clock, uploads, _) = make(1_000);
        bus.send(ProcessStarted).unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::Lifecycle {
                    kind: LifecycleKind::ProcessStarted
                }
            )),
            "expected ProcessStarted lifecycle upload"
        );
        drop(u);
        let s = get_state(&mut bus);
        assert_eq!(s.last_process_started, 1_000);
        assert_eq!(s.last_running_started, 1_000);
    }

    #[test]
    fn process_stopped_shutdown_emits_upload() {
        let (mut bus, _clock, uploads, _) = make(2_000);
        bus.send(ProcessStopped(ProcessStoppedReason::Shutdown))
            .unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::Lifecycle {
                    kind: LifecycleKind::ProcessStoppedShutdown
                }
            )),
            "expected ProcessStoppedShutdown lifecycle upload"
        );
        drop(u);
        let s = get_state(&mut bus);
        assert_eq!(s.last_process_stopped_shutdown, 2_000);
    }

    #[test]
    fn process_stopped_user_emits_upload_and_high_risk_alert() {
        let (mut bus, _clock, uploads, _) = make(4_000);
        bus.send(ProcessStopped(ProcessStoppedReason::User))
            .unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::Lifecycle {
                    kind: LifecycleKind::ProcessStoppedUser
                }
            )),
            "expected ProcessStoppedUser lifecycle upload"
        );
        let alert = u.iter().find(|e| {
            matches!(
                e.kind,
                UploadKind::LifecycleAlert {
                    reason: AlertReason::UserStoppedProcess
                }
            )
        });
        assert!(alert.is_some(), "expected UserStoppedProcess alert");
        assert!(alert.unwrap().risk >= 0.9, "alert should be high risk");
        drop(u);
        let s = get_state(&mut bus);
        assert_eq!(s.last_process_stopped_user, 4_000);
    }

    #[test]
    fn computer_suspended_emits_upload() {
        let (mut bus, _clock, uploads, _) = make(5_000);
        bus.send(ComputerSuspended).unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::Lifecycle {
                    kind: LifecycleKind::ComputerSuspended
                }
            )),
            "expected ComputerSuspended lifecycle upload"
        );
        drop(u);
        let s = get_state(&mut bus);
        assert!(matches!(s.status, LifecycleStatus::Suspended));
        assert_eq!(s.last_computer_suspend, 5_000);
    }

    #[test]
    fn computer_resumed_after_suspend_emits_upload() {
        let (mut bus, clock, uploads, _) = make(6_000);
        bus.send(ComputerSuspended).unwrap();
        bus.iter().unwrap();
        uploads.lock().unwrap().clear();
        clock.advance(1_000);
        bus.send(ComputerResumed).unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::Lifecycle {
                    kind: LifecycleKind::ComputerResumed
                }
            )),
            "expected ComputerResumed lifecycle upload"
        );
        drop(u);
        let s = get_state(&mut bus);
        assert!(matches!(s.status, LifecycleStatus::Running));
        assert_eq!(s.last_computer_resume, 7_000);
        assert_eq!(s.last_running_started, 7_000);
    }

    #[test]
    fn fourth_ping_while_suspended_triggers_missing_resume_alert() {
        let (mut bus, _clock, uploads, _) = make(1_000);
        bus.send(ComputerSuspended).unwrap();
        bus.iter().unwrap();
        uploads.lock().unwrap().clear();

        // 3 pings — counter increments to 3 but stays below the >3 threshold
        for _ in 0..3 {
            bus.send(Ping).unwrap();
            bus.iter().unwrap();
        }
        assert_eq!(get_state(&mut bus).pings_while_suspended, 3);
        uploads.lock().unwrap().clear();

        // 4th ping pushes counter past threshold, fires MissingResume and auto-sends ComputerResumed
        bus.send(Ping).unwrap();
        bus.iter().unwrap();

        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::LifecycleAlert {
                    reason: AlertReason::MissingResume
                }
            )),
            "expected MissingResume alert on 4th ping while suspended"
        );
        drop(u);
        let s = get_state(&mut bus);
        assert_eq!(
            s.pings_while_suspended, 0,
            "counter should reset after alert"
        );
        assert!(
            matches!(s.status, LifecycleStatus::Running),
            "auto-sent ComputerResumed should restore Running status"
        );
    }

    #[test]
    fn session_login_emits_lifecycle_upload() {
        let (mut bus, _clock, uploads, _) = make(1_000);
        bus.send(UserSessionLogin).unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::Lifecycle {
                    kind: LifecycleKind::Login
                }
            )),
            "expected Login lifecycle upload"
        );
        drop(u);
        let s = get_state(&mut bus);
        assert_eq!(s.last_login, 1_000);
    }

    #[test]
    fn session_logout_emits_high_risk_lifecycle_upload() {
        let (mut bus, _clock, uploads, _) = make(1_000);
        bus.send(UserSessionLogout).unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        let upload = u.iter().find(|e| {
            matches!(
                e.kind,
                UploadKind::Lifecycle {
                    kind: LifecycleKind::Logout
                }
            )
        });
        assert!(upload.is_some(), "expected Logout lifecycle upload");
        assert!(
            upload.unwrap().risk >= 0.9,
            "logout upload should be high risk"
        );
    }

    #[test]
    fn ping_gap_while_running_emits_high_risk_alert() {
        let (mut bus, clock, uploads, _) = make(1_000);
        // ProcessStarted initialises last_running_started; first Ping seeds last_ping.
        // Both happen within the 60 s login grace window so no spurious alerts fire.
        bus.send(ProcessStarted).unwrap();
        bus.iter().unwrap();
        bus.send(Ping).unwrap();
        bus.iter().unwrap();
        assert_eq!(get_state(&mut bus).last_ping, 1_000);
        uploads.lock().unwrap().clear();

        // Jump forward past both the 60 s login grace and the 10 s ping-gap threshold
        clock.set(100_000);
        bus.send(Ping).unwrap();
        bus.iter().unwrap();

        let u = uploads.lock().unwrap();
        let alert = u.iter().find(|e| {
            matches!(
                e.kind,
                UploadKind::LifecycleAlert {
                    reason: AlertReason::PingGapWhileRunning
                }
            )
        });
        assert!(alert.is_some(), "expected PingGapWhileRunning alert");
        assert!(
            alert.unwrap().risk >= 0.9,
            "ping gap alert should be high risk"
        );
        drop(u);
        assert_eq!(get_state(&mut bus).last_ping, 100_000);
    }

    #[test]
    fn ping_within_login_grace_period_does_not_emit_alert() {
        let (mut bus, clock, uploads, _) = make(1_000);
        // Establish last_running_started and last_ping at t=1_000
        bus.send(ProcessStarted).unwrap();
        bus.iter().unwrap();
        bus.send(Ping).unwrap();
        bus.iter().unwrap();
        uploads.lock().unwrap().clear();

        // User logs in at t=20_000; next ping at t=30_000 is within the 60 s grace window
        clock.set(20_000);
        bus.send(UserSessionLogin).unwrap();
        bus.iter().unwrap();
        assert_eq!(get_state(&mut bus).last_login, 20_000);

        clock.set(30_000);
        bus.send(Ping).unwrap();
        bus.iter().unwrap();

        let u = uploads.lock().unwrap();
        assert!(
            !u.iter().any(|e| matches!(
                e.kind,
                UploadKind::LifecycleAlert {
                    reason: AlertReason::PingGapWhileRunning
                }
            )),
            "ping gap alert should be suppressed within 60 s login grace window"
        );
    }

    #[test]
    fn process_killed_before_shutdown_emits_alert() {
        let (mut bus, clock, uploads, _) = make(1_000);
        // Process starts, then is killed unexpectedly (Other), then the computer shuts down cleanly
        bus.send(ProcessStarted).unwrap();
        bus.iter().unwrap();
        bus.send(ProcessStopped(ProcessStoppedReason::Other))
            .unwrap();
        bus.iter().unwrap();
        assert_eq!(get_state(&mut bus).last_process_stopped_other, 1_000);

        clock.set(12_000);
        bus.send(ProcessStopped(ProcessStoppedReason::Shutdown))
            .unwrap();
        bus.iter().unwrap();
        assert_eq!(get_state(&mut bus).last_process_stopped_shutdown, 12_000);
        uploads.lock().unwrap().clear();

        // On the next start the gap (12_000 - 1_000 = 11_000 ms) exceeds the 10 s threshold
        clock.set(20_000);
        bus.send(ProcessStarted).unwrap();
        bus.iter().unwrap();

        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::LifecycleAlert {
                    reason: AlertReason::ProcessKilledBeforeShutdown
                }
            )),
            "expected ProcessKilledBeforeShutdown alert"
        );
    }

    #[test]
    fn force_killed_process_with_platform_shutdown_emits_high_risk_alert() {
        // Scenario (all times in ms):
        //   01_000  ProcessStarted  (run 1)
        //   02_000  Ping, Ping      (run 1 active)
        //   10_000  ProcessStopped(Other)  (run 1 killed)
        //   12_000  ProcessStarted  (run 2)
        //   13–15k  Ping, Ping, Ping  (run 2 active)
        //   [20_000 force-killed — no event]
        //   [70_000 computer shuts down — detected only via platform hook]
        //   [100_000 computer boots]
        //   110_000 ProcessStarted  (run 3) → should emit high-risk alert

        // Build bus manually so we can keep a handle to the platform after boxing.
        let platform = TestPlatformHooks::new();
        platform.clock.set(1_000);
        let clock = platform.clock.clone();
        let platform_handle = platform.clone();

        let uploads: Arc<Mutex<Vec<Upload>>> = Arc::new(Mutex::new(Vec::new()));
        let u_ref = Arc::clone(&uploads);
        let module = LifecycleModule::new(Box::new(platform));
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &Upload| {
            u_ref.lock().unwrap().push(ev.clone());
            Ok(())
        });

        // Run 1
        bus.send(ProcessStarted).unwrap();
        bus.iter().unwrap();
        for t in [2_000i64, 3_000] {
            clock.set(t);
            bus.send(Ping).unwrap();
            bus.iter().unwrap();
        }
        clock.set(10_000);
        bus.send(ProcessStopped(ProcessStoppedReason::Other))
            .unwrap();
        bus.iter().unwrap();
        assert_eq!(get_state(&mut bus).last_process_stopped_other, 10_000);

        // Run 2 — process starts, pings, then is force-killed (no ProcessStopped event)
        clock.set(12_000);
        bus.send(ProcessStarted).unwrap();
        bus.iter().unwrap();
        for t in [13_000i64, 14_000, 15_000] {
            clock.set(t);
            bus.send(Ping).unwrap();
            bus.iter().unwrap();
        }
        assert_eq!(get_state(&mut bus).last_ping, 15_000);
        assert_eq!(
            get_state(&mut bus).last_process_stopped_shutdown,
            0,
            "no shutdown recorded yet"
        );

        // Computer shuts down at t=70_000 (55 s after last ping), boots at t=100_000.
        // Only the platform hook carries this information — no events were sent.
        platform_handle.set_last_shutdown_time(Some(70_000));
        platform_handle.set_last_startup_time(Some(100_000));
        uploads.lock().unwrap().clear();

        // Run 3 — process starts; lifecycle should detect the force-kill gap
        clock.set(110_000);
        bus.send(ProcessStarted).unwrap();
        bus.iter().unwrap();

        let u = uploads.lock().unwrap();
        let alert = u.iter().find(|e| {
            matches!(
                e.kind,
                UploadKind::LifecycleAlert {
                    reason: AlertReason::ForceKilledBeforeShutdown
                }
            )
        });
        assert!(
            alert.is_some(),
            "expected ForceKilledBeforeShutdown alert (55 s gap between last ping and shutdown)"
        );
        assert!(alert.unwrap().risk >= 0.9, "alert should be high risk");

        // The backfill path should emit a ProcessStoppedShutdown upload for the missing event
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::Lifecycle {
                    kind: LifecycleKind::ProcessStoppedShutdown
                }
            )),
            "expected backfilled ProcessStoppedShutdown upload"
        );
        // UnexpectedProcessStart should be suppressed: boot was only 10 s ago
        assert!(
            !u.iter().any(|e| matches!(
                e.kind,
                UploadKind::LifecycleAlert {
                    reason: AlertReason::UnexpectedProcessStart
                }
            )),
            "UnexpectedProcessStart should be suppressed by fresh-boot guard"
        );
        drop(u);

        let s = get_state(&mut bus);
        assert_eq!(
            s.last_process_stopped_shutdown, 70_000,
            "shutdown time should be backfilled into state"
        );
    }

    #[test]
    fn state_round_trips_through_save_and_load() {
        let (mut bus, clock, _, _) = make(30_000);
        // Build up recognisable state entirely through events
        bus.send(UserSessionLogin).unwrap(); // last_login = 30_000
        bus.iter().unwrap();

        clock.set(55_000);
        bus.send(ProcessStarted).unwrap(); // last_running_started = 55_000
        bus.iter().unwrap();

        clock.set(99_000);
        bus.send(Ping).unwrap(); // last_ping = 99_000 (first ping, so no gap alert)
        bus.iter().unwrap();

        bus.send(ComputerSuspended).unwrap(); // status = Suspended
        bus.iter().unwrap();
        bus.send(Ping).unwrap(); // pings_while_suspended = 1
        bus.iter().unwrap();
        bus.send(Ping).unwrap(); // pings_while_suspended = 2
        bus.iter().unwrap();

        let s = get_state(&mut bus);
        assert_eq!(s.last_login, 30_000);
        assert_eq!(s.last_running_started, 55_000);
        assert_eq!(s.last_ping, 99_000);
        assert_eq!(s.pings_while_suspended, 2);
        assert!(matches!(s.status, LifecycleStatus::Suspended));

        let saved = bus.save().unwrap();

        let platform2 = TestPlatformHooks::new();
        let module2 = LifecycleModule::new(Box::new(platform2));
        let mut bus2 = EventBus::new(vec![Box::new(module2)], saved).unwrap();

        let s2 = get_state(&mut bus2);
        assert_eq!(s2.last_login, 30_000);
        assert_eq!(s2.last_running_started, 55_000);
        assert_eq!(s2.last_ping, 99_000);
        assert_eq!(s2.pings_while_suspended, 2);
        assert!(matches!(s2.status, LifecycleStatus::Suspended));
    }
}
