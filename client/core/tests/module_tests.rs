//! Unit-level behavioral tests for each observer module.
//!
//! Each test creates an observer directly with a `mpsc::channel`, a
//! `TestPlatformHooks`, and/or a `MockApiClient`, then drives it with
//! `on_event()` calls and inspects the events that come out of the channel.
//! No `MonitorService` or `Scenario` required — every test is fully isolated.
//!
//! Run with:
//!   cargo test -p virtue-core --features testing --test module_tests

use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use virtue_core::events::{
    AlertReason, Event, LifecycleKind, Observer, PartialStatus, ProcessStoppedReason, UploadKind,
};
use virtue_core::model::{BatchRecipient, DeviceCredentials, DeviceSettings, LogEntry};
use virtue_core::module::auth::{AuthObserver, AuthObserverState};
use virtue_core::module::capture_availability::CaptureAvailabilityObserver;
use virtue_core::module::lifecycle::{LifecycleObserver, LifecycleStatus};
use virtue_core::module::request_handler::RequestObserver;
use virtue_core::module::screenshot::{ScreenshotConfig, ScreenshotObserver};
use virtue_core::module::status::StatusObserver;
use virtue_core::module::upload::{UploadConfig, UploadObserver};
use virtue_core::testing::{MockApiClient, MockClock, TestPlatformHooks};
use virtue_core::CoreError;

// ── Shared helpers ─────────────────────────────────────────────────────────────

fn drain(rx: &Receiver<Event>) -> Vec<Event> {
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    events
}

fn platform_at(ms: i64) -> TestPlatformHooks {
    let clock = MockClock::new(ms);
    TestPlatformHooks::with_clock(clock)
}

fn valid_settings() -> DeviceSettings {
    DeviceSettings {
        device_id: "test-device".into(),
        name: "test device".into(),
        platform: "test".into(),
        // X25519 base point (u=9): valid curve point accepted by HPKE
        owner: Some(BatchRecipient {
            user_id: "test-user".into(),
            pub_key_base64: "CQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
        }),
        partners: Vec::new(),
        hash_base_url: None,
    }
}

fn valid_credentials() -> DeviceCredentials {
    DeviceCredentials {
        device_id: "test-device".into(),
        access_token: "test-access".into(),
        refresh_token: "test-refresh".into(),
    }
}

fn login_event() -> Event {
    Event::Login {
        credentials: valid_credentials(),
        settings: valid_settings(),
    }
}

// ── LifecycleObserver ──────────────────────────────────────────────────────────

mod lifecycle {
    use super::*;

    fn make(platform: TestPlatformHooks) -> (LifecycleObserver, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        (LifecycleObserver::new(Box::new(platform), tx), rx)
    }

    #[test]
    fn status_request_reports_is_running_and_last_ping() {
        let platform = platform_at(1000);
        let (mut obs, rx) = make(platform);
        obs.state.last_ping = 500;
        obs.on_event(&Event::StatusRequest).unwrap();
        let events = drain(&rx);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Event::PartialStatus(PartialStatus::Lifecycle {
                is_running,
                last_loop_at_ms,
            }) => {
                assert!(is_running);
                assert_eq!(*last_loop_at_ms, Some(500));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn status_request_without_prior_ping_has_no_timestamp() {
        let platform = platform_at(1000);
        let (mut obs, rx) = make(platform);
        obs.on_event(&Event::StatusRequest).unwrap();
        let events = drain(&rx);
        match &events[0] {
            Event::PartialStatus(PartialStatus::Lifecycle { last_loop_at_ms, .. }) => {
                assert_eq!(*last_loop_at_ms, None);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn process_started_emits_lifecycle_upload_and_updates_timestamps() {
        let platform = platform_at(1000);
        let (mut obs, rx) = make(platform);
        obs.on_event(&Event::ProcessStarted).unwrap();
        let events = drain(&rx);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::Upload {
                    kind: UploadKind::Lifecycle {
                        kind: LifecycleKind::ProcessStarted,
                    },
                    ..
                }
            )),
            "expected ProcessStarted lifecycle upload"
        );
        assert_eq!(obs.state.last_process_started, 1000);
        assert_eq!(obs.state.last_running_started, 1000);
    }

    #[test]
    fn process_stopped_shutdown_emits_upload_and_updates_state() {
        let platform = platform_at(2000);
        let (mut obs, rx) = make(platform);
        obs.on_event(&Event::ProcessStopped(ProcessStoppedReason::Shutdown))
            .unwrap();
        let events = drain(&rx);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Upload {
                kind: UploadKind::Lifecycle {
                    kind: LifecycleKind::ProcessStoppedShutdown,
                },
                ..
            }
        )));
        assert_eq!(obs.state.last_process_stopped_shutdown, 2000);
    }

    #[test]
    fn process_stopped_other_emits_upload_and_updates_state() {
        let platform = platform_at(3000);
        let (mut obs, rx) = make(platform);
        obs.on_event(&Event::ProcessStopped(ProcessStoppedReason::Other))
            .unwrap();
        let events = drain(&rx);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Upload {
                kind: UploadKind::Lifecycle {
                    kind: LifecycleKind::ProcessStoppedOther,
                },
                ..
            }
        )));
        assert_eq!(obs.state.last_process_stopped_other, 3000);
    }

    #[test]
    fn process_stopped_user_emits_lifecycle_upload_and_high_risk_alert() {
        let platform = platform_at(4000);
        let (mut obs, rx) = make(platform);
        obs.on_event(&Event::ProcessStopped(ProcessStoppedReason::User))
            .unwrap();
        let events = drain(&rx);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Upload {
                kind: UploadKind::Lifecycle {
                    kind: LifecycleKind::ProcessStoppedUser,
                },
                ..
            }
        )));
        let alert = events.iter().find(|e| {
            matches!(
                e,
                Event::Upload {
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::UserStoppedProcess,
                    },
                    ..
                }
            )
        });
        assert!(alert.is_some(), "expected UserStoppedProcess alert");
        if let Some(Event::Upload { risk, .. }) = alert {
            assert!(*risk >= 0.9, "user-stop alert should be high risk, got {risk}");
        }
        assert_eq!(obs.state.last_process_stopped_user, 4000);
    }

    #[test]
    fn computer_suspended_emits_upload_and_transitions_state() {
        let platform = platform_at(5000);
        let (mut obs, rx) = make(platform);
        obs.on_event(&Event::ComputerSuspended).unwrap();
        let events = drain(&rx);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Upload {
                kind: UploadKind::Lifecycle {
                    kind: LifecycleKind::ComputerSuspended,
                },
                ..
            }
        )));
        assert!(
            matches!(obs.state.status, LifecycleStatus::Suspended),
            "should be in Suspended state after ComputerSuspended"
        );
        assert_eq!(obs.state.last_computer_suspend, 5000);
    }

    #[test]
    fn computer_resumed_while_suspended_emits_upload_and_restores_running() {
        let platform = platform_at(6000);
        let (mut obs, rx) = make(platform.clone());
        obs.on_event(&Event::ComputerSuspended).unwrap();
        drain(&rx);

        platform.clock.set(7000);
        obs.on_event(&Event::ComputerResumed).unwrap();
        let events = drain(&rx);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Upload {
                kind: UploadKind::Lifecycle {
                    kind: LifecycleKind::ComputerResumed,
                },
                ..
            }
        )));
        assert!(
            matches!(obs.state.status, LifecycleStatus::Running),
            "should be Running after ComputerResumed"
        );
        assert_eq!(obs.state.last_computer_resume, 7000);
        assert_eq!(obs.state.last_running_started, 7000);
        assert_eq!(obs.state.pings_while_suspended, 0);
    }

    #[test]
    fn three_pings_while_suspended_do_not_trigger_alert() {
        let platform = platform_at(1000);
        let (mut obs, rx) = make(platform);
        obs.state.status = LifecycleStatus::Suspended;
        for _ in 0..3 {
            obs.on_event(&Event::Ping).unwrap();
        }
        let events = drain(&rx);
        assert_eq!(obs.state.pings_while_suspended, 3);
        assert!(
            !events.iter().any(|e| matches!(
                e,
                Event::Upload {
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::MissingResume,
                    },
                    ..
                }
            )),
            "3 pings while suspended should not trigger MissingResume alert"
        );
    }

    #[test]
    fn fourth_ping_while_suspended_triggers_missing_resume_alert_and_synthetic_resume() {
        let platform = platform_at(1000);
        let (mut obs, rx) = make(platform);
        obs.state.status = LifecycleStatus::Suspended;
        obs.state.pings_while_suspended = 3;
        obs.on_event(&Event::Ping).unwrap();
        let events = drain(&rx);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::Upload {
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::MissingResume,
                    },
                    ..
                }
            )),
            "expected MissingResume alert"
        );
        assert!(
            events.iter().any(|e| matches!(e, Event::ComputerResumed)),
            "expected synthetic ComputerResumed event"
        );
        assert_eq!(obs.state.pings_while_suspended, 0, "counter should be reset");
    }

    #[test]
    fn ping_gap_while_running_emits_high_risk_alert() {
        let platform = platform_at(100_000);
        let (mut obs, rx) = make(platform);
        obs.state.last_login = 0; // > 60s ago at t=100_000
        obs.state.last_ping = 1_000; // 99s ago
        obs.state.last_running_started = 1_000;
        obs.on_event(&Event::Ping).unwrap();
        let events = drain(&rx);
        let alert = events.iter().find(|e| {
            matches!(
                e,
                Event::Upload {
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::PingGapWhileRunning,
                    },
                    ..
                }
            )
        });
        assert!(alert.is_some(), "expected PingGapWhileRunning alert");
        if let Some(Event::Upload { risk, .. }) = alert {
            assert!(*risk >= 0.9, "ping gap alert should be high risk, got {risk}");
        }
        assert_eq!(obs.state.last_ping, 100_000);
    }

    #[test]
    fn ping_within_login_grace_period_does_not_emit_alert() {
        let platform = platform_at(30_000);
        let (mut obs, rx) = make(platform);
        obs.state.last_login = 20_000; // only 10s ago — within 60s grace
        obs.state.last_ping = 1_000;
        obs.state.last_running_started = 1_000;
        obs.on_event(&Event::Ping).unwrap();
        let events = drain(&rx);
        assert!(
            !events.iter().any(|e| matches!(
                e,
                Event::Upload {
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::PingGapWhileRunning,
                    },
                    ..
                }
            )),
            "ping gap alert should be suppressed within 60s login grace window"
        );
    }

    #[test]
    fn process_killed_before_shutdown_emits_alert() {
        // last_process_stopped_other=1_000, last_process_stopped_shutdown=12_000
        // → gap between stop and shutdown = 11_000 > 10_000 → alert fires on ProcessStarted
        let platform = platform_at(20_000);
        let (mut obs, rx) = make(platform);
        obs.state.last_process_stopped_other = 1_000;
        obs.state.last_process_stopped_shutdown = 12_000;
        obs.on_event(&Event::ProcessStarted).unwrap();
        let events = drain(&rx);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::Upload {
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::ProcessKilledBeforeShutdown,
                    },
                    ..
                }
            )),
            "expected ProcessKilledBeforeShutdown alert"
        );
    }

    #[test]
    fn unexpected_process_start_after_long_ping_gap_emits_alert() {
        // ping_gap=99_000 > 10_000; boot=None → now_ms-boot=100_000 > 10_000
        // last_process_started must be non-zero (indicates prior run)
        // login > 60s ago
        let platform = platform_at(100_000);
        let (mut obs, rx) = make(platform);
        obs.state.last_ping = 1_000;
        obs.state.last_process_started = 1; // prior run existed
        obs.state.last_login = 0;
        obs.on_event(&Event::ProcessStarted).unwrap();
        let events = drain(&rx);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::Upload {
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::UnexpectedProcessStart,
                    },
                    ..
                }
            )),
            "expected UnexpectedProcessStart alert"
        );
    }

    #[test]
    fn first_ever_process_start_does_not_emit_unexpected_start_alert() {
        // last_process_started=0 (never ran before) → alert suppressed
        let platform = platform_at(100_000);
        let (mut obs, rx) = make(platform);
        obs.state.last_ping = 1_000;
        obs.state.last_process_started = 0; // no prior run
        obs.state.last_login = 0;
        obs.on_event(&Event::ProcessStarted).unwrap();
        let events = drain(&rx);
        assert!(
            !events.iter().any(|e| matches!(
                e,
                Event::Upload {
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::UnexpectedProcessStart,
                    },
                    ..
                }
            )),
            "first process start should not fire UnexpectedProcessStart"
        );
    }

    #[test]
    fn user_stop_requested_sets_flag_and_take_clears_it() {
        let platform = platform_at(1000);
        let (mut obs, _rx) = make(platform);
        assert!(!obs.take_user_stop_requested(), "flag should start false");
        obs.on_event(&Event::UserStopRequested {
            source: "tray".into(),
        })
        .unwrap();
        assert!(obs.take_user_stop_requested(), "flag should be set after UserStopRequested");
        assert!(
            !obs.take_user_stop_requested(),
            "flag should be cleared after take"
        );
    }

    #[test]
    fn session_login_emits_lifecycle_upload_and_updates_last_login() {
        let platform = platform_at(1000);
        let (mut obs, rx) = make(platform);
        obs.on_event(&Event::UserSessionLogin).unwrap();
        let events = drain(&rx);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Upload {
                kind: UploadKind::Lifecycle {
                    kind: LifecycleKind::Login,
                },
                ..
            }
        )));
        assert_eq!(obs.state.last_login, 1000);
    }

    #[test]
    fn session_logout_emits_high_risk_lifecycle_upload() {
        let platform = platform_at(1000);
        let (mut obs, rx) = make(platform);
        obs.on_event(&Event::UserSessionLogout).unwrap();
        let events = drain(&rx);
        let upload = events.iter().find(|e| {
            matches!(
                e,
                Event::Upload {
                    kind: UploadKind::Lifecycle {
                        kind: LifecycleKind::Logout,
                    },
                    ..
                }
            )
        });
        assert!(upload.is_some(), "expected Logout lifecycle upload");
        if let Some(Event::Upload { risk, .. }) = upload {
            assert!(*risk >= 0.9, "logout upload should be high risk, got {risk}");
        }
    }

    #[test]
    fn process_stopped_while_suspended_emits_upload_and_updates_state() {
        let platform = platform_at(8000);
        let (mut obs, rx) = make(platform);
        obs.state.status = LifecycleStatus::Suspended;
        obs.on_event(&Event::ProcessStopped(ProcessStoppedReason::Shutdown))
            .unwrap();
        let events = drain(&rx);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Upload {
                kind: UploadKind::Lifecycle {
                    kind: LifecycleKind::ProcessStoppedShutdown,
                },
                ..
            }
        )));
        assert_eq!(obs.state.last_process_stopped_shutdown, 8000);
    }

    #[test]
    fn session_logout_while_suspended_emits_high_risk_upload() {
        let platform = platform_at(1000);
        let (mut obs, rx) = make(platform);
        obs.state.status = LifecycleStatus::Suspended;
        obs.on_event(&Event::UserSessionLogout).unwrap();
        let events = drain(&rx);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Upload {
                kind: UploadKind::Lifecycle {
                    kind: LifecycleKind::Logout,
                },
                ..
            }
        )));
    }

    #[test]
    fn state_round_trips_through_save_and_load() {
        let platform = platform_at(1000);
        let (mut obs, _rx) = make(platform);
        obs.state.last_login = 42_000;
        obs.state.last_ping = 99_000;
        obs.state.user_stop_requested = true;
        obs.state.pings_while_suspended = 2;
        obs.state.last_running_started = 55_000;
        let saved = obs.save_state().unwrap();

        let (mut obs2, _rx2) = make(platform_at(1000));
        obs2.load_state(saved).unwrap();
        assert_eq!(obs2.state.last_login, 42_000);
        assert_eq!(obs2.state.last_ping, 99_000);
        assert!(obs2.state.user_stop_requested);
        assert_eq!(obs2.state.pings_while_suspended, 2);
        assert_eq!(obs2.state.last_running_started, 55_000);
    }

    #[test]
    fn ping_updates_last_ping_timestamp() {
        let platform = platform_at(5000);
        let (mut obs, _rx) = make(platform);
        obs.on_event(&Event::Ping).unwrap();
        assert_eq!(obs.state.last_ping, 5000);
    }
}

// ── ScreenshotObserver ─────────────────────────────────────────────────────────

mod screenshot {
    use super::*;

    fn make(platform: TestPlatformHooks) -> (ScreenshotObserver, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let config = ScreenshotConfig {
            screenshot_interval: Duration::from_secs(60),
        };
        (ScreenshotObserver::new(Box::new(platform), tx, config), rx)
    }

    #[test]
    fn ping_when_unauthenticated_does_nothing() {
        let platform = platform_at(1000);
        let (mut obs, rx) = make(platform.clone());
        obs.on_event(&Event::Ping).unwrap();
        assert!(drain(&rx).is_empty());
        assert_eq!(platform.take_call_count(), 0);
    }

    #[test]
    fn login_sets_authenticated_and_clears_schedule() {
        let platform = platform_at(1000);
        let (mut obs, _rx) = make(platform);
        obs.state.last_screenshot_at_ms = Some(500);
        obs.on_event(&login_event()).unwrap();
        assert!(obs.state.authenticated, "login should set authenticated");
        assert_eq!(
            obs.state.last_screenshot_at_ms, None,
            "login should clear the screenshot schedule"
        );
    }

    #[test]
    fn logout_clears_authenticated_and_schedule() {
        let platform = platform_at(1000);
        let (mut obs, _rx) = make(platform);
        obs.state.authenticated = true;
        obs.state.last_screenshot_at_ms = Some(500);
        obs.on_event(&Event::Logout).unwrap();
        assert!(!obs.state.authenticated);
        assert_eq!(obs.state.last_screenshot_at_ms, None);
    }

    #[test]
    fn first_ping_when_authenticated_takes_screenshot_immediately() {
        let platform = platform_at(1000);
        let (mut obs, rx) = make(platform.clone());
        obs.state.authenticated = true;
        obs.on_event(&Event::Ping).unwrap();
        let events = drain(&rx);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::Upload {
                    kind: UploadKind::Screenshot { .. },
                    ..
                }
            )),
            "expected Screenshot upload on first ping"
        );
        assert_eq!(platform.take_call_count(), 1);
        assert_eq!(obs.state.last_screenshot_at_ms, Some(1000));
    }

    #[test]
    fn screenshot_not_retaken_before_interval_elapses() {
        let platform = platform_at(30_000);
        let (mut obs, rx) = make(platform.clone());
        obs.state.authenticated = true;
        obs.state.last_screenshot_at_ms = Some(0);
        obs.on_event(&Event::Ping).unwrap(); // only 30s elapsed, interval is 60s
        drain(&rx);
        assert_eq!(
            platform.take_call_count(),
            0,
            "screenshot should not be taken before 60s interval"
        );
    }

    #[test]
    fn screenshot_retaken_after_interval_elapses() {
        let platform = platform_at(61_000);
        let (mut obs, rx) = make(platform.clone());
        obs.state.authenticated = true;
        obs.state.last_screenshot_at_ms = Some(0);
        obs.on_event(&Event::Ping).unwrap(); // 61s elapsed, interval is 60s
        let events = drain(&rx);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::Upload {
                    kind: UploadKind::Screenshot { .. },
                    ..
                }
            )),
            "expected screenshot after interval elapsed"
        );
        assert_eq!(platform.take_call_count(), 1);
    }

    #[test]
    fn backwards_clock_resets_schedule_and_takes_screenshot() {
        let platform = platform_at(10_000);
        let (mut obs, rx) = make(platform.clone());
        obs.state.authenticated = true;
        obs.state.last_screenshot_at_ms = Some(50_000); // last screenshot is in the "future"
        obs.on_event(&Event::Ping).unwrap();
        let events = drain(&rx);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::Upload {
                    kind: UploadKind::Screenshot { .. },
                    ..
                }
            )),
            "backwards clock should reset schedule and take screenshot"
        );
        assert_eq!(platform.take_call_count(), 1);
    }

    #[test]
    fn screenshot_failure_emits_capture_failed_event() {
        let platform = platform_at(1000);
        platform.queue_screenshot(Err(CoreError::CommandFailed("no display".into())));
        let (mut obs, rx) = make(platform.clone());
        obs.state.authenticated = true;
        let _ = obs.on_event(&Event::Ping); // returns Err but we care about the queued event
        let events = drain(&rx);
        assert!(
            events.iter().any(|e| matches!(e, Event::CaptureFailed)),
            "screenshot failure should emit CaptureFailed"
        );
        assert_eq!(platform.take_call_count(), 1);
    }

    #[test]
    fn non_ping_and_non_auth_events_are_ignored() {
        let platform = platform_at(1000);
        let (mut obs, rx) = make(platform.clone());
        obs.state.authenticated = true;
        for event in [
            Event::ComputerSuspended,
            Event::ComputerResumed,
            Event::ProcessStarted,
            Event::StatusRequest,
            Event::CaptureFailed,
        ] {
            obs.on_event(&event).unwrap();
        }
        assert!(drain(&rx).is_empty(), "non-ping events should be no-ops");
        assert_eq!(platform.take_call_count(), 0);
    }

    #[test]
    fn state_round_trips_through_save_and_load() {
        let platform = platform_at(1000);
        let (mut obs, _rx) = make(platform);
        obs.state.authenticated = true;
        obs.state.last_screenshot_at_ms = Some(12345);
        let saved = obs.save_state().unwrap();

        let (mut obs2, _rx2) = make(platform_at(1000));
        obs2.load_state(saved).unwrap();
        assert!(obs2.state.authenticated);
        assert_eq!(obs2.state.last_screenshot_at_ms, Some(12345));
    }

    #[test]
    fn load_state_unauthenticated_clears_schedule() {
        let platform = platform_at(1000);
        let (mut obs, _rx) = make(platform);
        obs.state.authenticated = false;
        obs.state.last_screenshot_at_ms = Some(999);
        let saved = obs.save_state().unwrap();

        let (mut obs2, _rx2) = make(platform_at(1000));
        obs2.load_state(saved).unwrap();
        assert_eq!(
            obs2.state.last_screenshot_at_ms, None,
            "schedule must be cleared when loading unauthenticated state"
        );
    }
}

// ── AuthObserver ───────────────────────────────────────────────────────────────

mod auth {
    use super::*;

    fn make(api: MockApiClient) -> (AuthObserver<MockApiClient>, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let obs = AuthObserver::new(api, "test-device".into(), "test-platform".into(), tx);
        (obs, rx)
    }

    fn make_api_with_settings() -> MockApiClient {
        let api = MockApiClient::new();
        api.state().default_device_settings = valid_settings();
        api
    }

    #[test]
    fn login_requested_success_emits_login_and_login_result() {
        let api = make_api_with_settings();
        let (mut obs, rx) = make(api.clone());
        obs.on_event(&Event::LoginRequested {
            email: "alice@test.com".into(),
            password: "secret".into(),
        })
        .unwrap();
        let events = drain(&rx);
        assert!(
            events.iter().any(|e| matches!(e, Event::Login { .. })),
            "successful login should emit Login event"
        );
        assert!(
            events.iter().any(|e| matches!(e, Event::LoginResult { success: true, .. })),
            "successful login should emit LoginResult with success=true"
        );
        // Verify API calls happened
        assert_eq!(api.state().login_calls.len(), 1);
        assert_eq!(api.state().register_device_calls.len(), 1);
    }

    #[test]
    fn login_requested_failure_emits_error_result_no_login_event() {
        let api = MockApiClient::new();
        api.program_login(Err(CoreError::HttpStatus {
            status: 401,
            message: "bad credentials".into(),
        }));
        let (mut obs, rx) = make(api);
        obs.on_event(&Event::LoginRequested {
            email: "alice@test.com".into(),
            password: "wrong".into(),
        })
        .unwrap();
        let events = drain(&rx);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::LoginResult {
                    success: false,
                    error: Some(_),
                    ..
                }
            )),
            "failed login should emit LoginResult with success=false"
        );
        assert!(
            !events.iter().any(|e| matches!(e, Event::Login { .. })),
            "failed login must not emit Login event"
        );
    }

    #[test]
    fn logout_requested_calls_api_and_emits_logout_and_result() {
        let api = MockApiClient::new();
        let (mut obs, rx) = make(api.clone());
        // Pre-seed credentials so logout calls the API
        let state_json = serde_json::to_value(AuthObserverState {
            user_access_token: Some("u-tok".into()),
            device_credentials: Some(valid_credentials()),
        })
        .unwrap();
        obs.load_state(state_json).unwrap();
        drain(&rx); // discard any events from load

        obs.on_event(&Event::LogoutRequested).unwrap();
        let events = drain(&rx);
        assert!(
            events.iter().any(|e| matches!(e, Event::Logout)),
            "logout should emit Logout event"
        );
        assert!(
            events.iter().any(|e| matches!(e, Event::LogoutResult { success: true, .. })),
            "logout should emit successful LogoutResult"
        );
        assert_eq!(
            api.state().logout_calls.len(),
            1,
            "should have called API logout with the user token"
        );
    }

    #[test]
    fn logout_without_credentials_does_not_call_api() {
        let api = MockApiClient::new();
        let (mut obs, rx) = make(api.clone());
        obs.on_event(&Event::LogoutRequested).unwrap();
        let events = drain(&rx);
        assert!(events.iter().any(|e| matches!(e, Event::Logout)));
        assert_eq!(
            api.state().logout_calls.len(),
            0,
            "should not call API logout when there is no user token"
        );
    }

    #[test]
    fn ping_does_nothing_when_no_credentials() {
        let api = MockApiClient::new();
        let (mut obs, rx) = make(api.clone());
        obs.on_event(&Event::Ping).unwrap();
        assert!(drain(&rx).is_empty());
        assert!(api.state().get_device_settings_calls.is_empty());
    }

    #[test]
    fn ping_refreshes_settings_when_credentials_loaded_from_state() {
        let api = make_api_with_settings();
        let (mut obs, rx) = make(api.clone());
        // load_state with device_credentials sets needs_settings_refresh=true
        let state_json = serde_json::to_value(AuthObserverState {
            user_access_token: Some("u-tok".into()),
            device_credentials: Some(valid_credentials()),
        })
        .unwrap();
        obs.load_state(state_json).unwrap();
        drain(&rx);

        obs.on_event(&Event::Ping).unwrap();
        let events = drain(&rx);
        assert!(
            events.iter().any(|e| matches!(e, Event::DeviceSettingsRefreshed { .. })),
            "ping should refresh device settings when needed"
        );
        assert!(!api.state().get_device_settings_calls.is_empty());
    }

    #[test]
    fn status_request_when_unauthenticated_emits_auth_partial_status() {
        let api = MockApiClient::new();
        let (mut obs, rx) = make(api);
        obs.on_event(&Event::StatusRequest).unwrap();
        let events = drain(&rx);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Event::PartialStatus(PartialStatus::Auth {
                is_authenticated,
                device_id,
            }) => {
                assert!(!is_authenticated);
                assert_eq!(*device_id, None);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn status_request_when_authenticated_reports_device_id() {
        let api = MockApiClient::new();
        let (mut obs, rx) = make(api);
        let state_json = serde_json::to_value(AuthObserverState {
            user_access_token: None,
            device_credentials: Some(valid_credentials()),
        })
        .unwrap();
        obs.load_state(state_json).unwrap();
        drain(&rx);

        obs.on_event(&Event::StatusRequest).unwrap();
        let events = drain(&rx);
        match &events[0] {
            Event::PartialStatus(PartialStatus::Auth {
                is_authenticated,
                device_id,
            }) => {
                assert!(is_authenticated);
                assert_eq!(*device_id, Some("test-device".into()));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn state_round_trips_through_save_and_load() {
        let api = MockApiClient::new();
        let (mut obs, _rx) = make(api.clone());
        let state_json = serde_json::to_value(AuthObserverState {
            user_access_token: Some("u-tok".into()),
            device_credentials: Some(valid_credentials()),
        })
        .unwrap();
        obs.load_state(state_json).unwrap();

        let saved = obs.save_state().unwrap();
        let (mut obs2, _rx2) = make(MockApiClient::new());
        obs2.load_state(saved).unwrap();
        // Verify by checking status request reflects the loaded credentials
        let (tx, rx2) = mpsc::channel::<Event>();
        // Can't easily replace the sender, so we verify via ping (which refreshes settings when creds exist)
        let new_api = make_api_with_settings();
        let (mut obs3, rx3) = make(new_api);
        obs3.load_state(obs2.save_state().unwrap()).unwrap();
        obs3.on_event(&Event::Ping).unwrap();
        let events = drain(&rx3);
        assert!(
            events.iter().any(|e| matches!(e, Event::DeviceSettingsRefreshed { .. })),
            "credentials should survive save/load round-trip"
        );
        drop(tx);
        drop(rx2);
    }
}

// ── StatusObserver ─────────────────────────────────────────────────────────────

mod status {
    use super::*;

    fn make(expected_count: usize) -> (StatusObserver, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        (StatusObserver::new(expected_count, tx), rx)
    }

    #[test]
    fn one_partial_with_expected_one_triggers_response() {
        let (mut obs, rx) = make(1);
        obs.on_event(&Event::StatusRequest).unwrap();
        obs.on_event(&Event::PartialStatus(PartialStatus::Auth {
            is_authenticated: true,
            device_id: Some("dev".into()),
        }))
        .unwrap();
        let events = drain(&rx);
        assert!(
            events.iter().any(|e| matches!(e, Event::StatusResponse { .. })),
            "expected StatusResponse"
        );
    }

    #[test]
    fn response_only_after_all_expected_fragments_received() {
        let (mut obs, rx) = make(3);
        obs.on_event(&Event::StatusRequest).unwrap();

        obs.on_event(&Event::PartialStatus(PartialStatus::Auth {
            is_authenticated: false,
            device_id: None,
        }))
        .unwrap();
        assert!(drain(&rx).is_empty(), "should not respond after 1 of 3");

        obs.on_event(&Event::PartialStatus(PartialStatus::Lifecycle {
            is_running: true,
            last_loop_at_ms: Some(1000),
        }))
        .unwrap();
        assert!(drain(&rx).is_empty(), "should not respond after 2 of 3");

        obs.on_event(&Event::PartialStatus(PartialStatus::Upload {
            pending_request_count: 7,
        }))
        .unwrap();
        let events = drain(&rx);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Event::StatusResponse { status } => {
                assert!(!status.is_authenticated);
                assert!(status.is_running);
                assert_eq!(status.last_loop_at_ms, Some(1000));
                assert_eq!(status.pending_request_count, 7);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn each_partial_type_merges_into_response_correctly() {
        let (mut obs, rx) = make(3);
        obs.on_event(&Event::StatusRequest).unwrap();
        obs.on_event(&Event::PartialStatus(PartialStatus::Auth {
            is_authenticated: true,
            device_id: Some("my-device".into()),
        }))
        .unwrap();
        obs.on_event(&Event::PartialStatus(PartialStatus::Lifecycle {
            is_running: true,
            last_loop_at_ms: Some(42),
        }))
        .unwrap();
        obs.on_event(&Event::PartialStatus(PartialStatus::Upload {
            pending_request_count: 3,
        }))
        .unwrap();
        let events = drain(&rx);
        match &events[0] {
            Event::StatusResponse { status } => {
                assert!(status.is_authenticated);
                assert_eq!(status.device_id, Some("my-device".into()));
                assert!(status.is_running);
                assert_eq!(status.last_loop_at_ms, Some(42));
                assert_eq!(status.pending_request_count, 3);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn new_status_request_resets_accumulated_state() {
        let (mut obs, rx) = make(1);
        // First request
        obs.on_event(&Event::StatusRequest).unwrap();
        obs.on_event(&Event::PartialStatus(PartialStatus::Auth {
            is_authenticated: true,
            device_id: Some("dev1".into()),
        }))
        .unwrap();
        drain(&rx);

        // Second request — should see fresh defaults, not leftovers from first
        obs.on_event(&Event::StatusRequest).unwrap();
        obs.on_event(&Event::PartialStatus(PartialStatus::Auth {
            is_authenticated: false,
            device_id: None,
        }))
        .unwrap();
        let events = drain(&rx);
        match &events[0] {
            Event::StatusResponse { status } => {
                assert!(
                    !status.is_authenticated,
                    "second request should reflect new partial status, not first"
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn save_and_load_state_are_no_ops() {
        let (mut obs, _rx) = make(3);
        let state = obs.save_state().unwrap();
        assert!(state.is_null());
        obs.load_state(serde_json::Value::Null).unwrap();
    }
}

// ── CaptureAvailabilityObserver ────────────────────────────────────────────────

mod capture_availability {
    use super::*;

    fn make(platform: TestPlatformHooks) -> (CaptureAvailabilityObserver, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        (CaptureAvailabilityObserver::new(tx, Box::new(platform)), rx)
    }

    #[test]
    fn four_failures_below_threshold_no_upload() {
        let platform = platform_at(1000);
        let (mut obs, rx) = make(platform);
        for _ in 0..4 {
            obs.on_event(&Event::CaptureFailed).unwrap();
        }
        let events = drain(&rx);
        assert!(
            events.is_empty(),
            "4 failures should not trigger an upload"
        );
        assert_eq!(obs.state.recent_failures_ms.len(), 4);
    }

    #[test]
    fn fifth_failure_triggers_capture_failed_upload() {
        let platform = platform_at(1000);
        let (mut obs, rx) = make(platform);
        for _ in 0..5 {
            obs.on_event(&Event::CaptureFailed).unwrap();
        }
        let events = drain(&rx);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::Upload {
                    kind: UploadKind::CaptureFailed,
                    ..
                }
            )),
            "5 failures should trigger a CaptureFailed upload"
        );
        assert_eq!(
            obs.state.recent_failures_ms.len(),
            0,
            "failure list should be cleared after threshold"
        );
    }

    #[test]
    fn failures_older_than_window_expire_and_dont_count() {
        let platform = platform_at(0);
        let (mut obs, rx) = make(platform.clone());
        for _ in 0..4 {
            obs.on_event(&Event::CaptureFailed).unwrap();
        }
        drain(&rx);

        // Advance past the 30-minute window
        platform.clock.set(31 * 60 * 1000);
        obs.on_event(&Event::CaptureFailed).unwrap();
        let events = drain(&rx);
        assert!(
            events.is_empty(),
            "failures older than 30 minutes should not count toward threshold"
        );
        assert_eq!(
            obs.state.recent_failures_ms.len(),
            1,
            "only the new failure should remain after window expiry"
        );
    }

    #[test]
    fn failures_just_inside_window_still_count() {
        let platform = platform_at(0);
        let (mut obs, rx) = make(platform.clone());
        for _ in 0..4 {
            obs.on_event(&Event::CaptureFailed).unwrap();
        }
        drain(&rx);

        // Just inside the 30-minute window
        platform.clock.set(29 * 60 * 1000 + 999);
        obs.on_event(&Event::CaptureFailed).unwrap();
        let events = drain(&rx);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::Upload {
                    kind: UploadKind::CaptureFailed,
                    ..
                }
            )),
            "5 failures within the 30-minute window should trigger upload"
        );
    }

    #[test]
    fn non_capture_failed_events_are_completely_ignored() {
        let platform = platform_at(1000);
        let (mut obs, rx) = make(platform);
        obs.on_event(&Event::Ping).unwrap();
        obs.on_event(&Event::ProcessStarted).unwrap();
        obs.on_event(&Event::ComputerSuspended).unwrap();
        obs.on_event(&Event::UserSessionLogin).unwrap();
        obs.on_event(&Event::StatusRequest).unwrap();
        let events = drain(&rx);
        assert!(events.is_empty());
        assert_eq!(obs.state.recent_failures_ms.len(), 0);
    }

    #[test]
    fn state_round_trips_through_save_and_load() {
        let platform = platform_at(1000);
        let (mut obs, _rx) = make(platform);
        obs.state.recent_failures_ms = vec![100, 200, 300];
        let saved = obs.save_state().unwrap();

        let (mut obs2, _rx2) = make(platform_at(1000));
        obs2.load_state(saved).unwrap();
        assert_eq!(obs2.state.recent_failures_ms, vec![100, 200, 300]);
    }

    #[test]
    fn counter_resets_after_each_threshold_trigger() {
        // Verify: after 5 failures, counter resets so the NEXT 5 are needed to trigger again.
        let platform = platform_at(1000);
        let (mut obs, rx) = make(platform);
        for _ in 0..5 {
            obs.on_event(&Event::CaptureFailed).unwrap();
        }
        let first_batch = drain(&rx);
        assert!(first_batch.iter().any(|e| matches!(
            e,
            Event::Upload {
                kind: UploadKind::CaptureFailed,
                ..
            }
        )));

        // Now 4 more failures — should not trigger
        for _ in 0..4 {
            obs.on_event(&Event::CaptureFailed).unwrap();
        }
        assert!(
            drain(&rx).is_empty(),
            "after reset, 4 more failures should not trigger"
        );

        // 5th failure triggers again
        obs.on_event(&Event::CaptureFailed).unwrap();
        let second_batch = drain(&rx);
        assert!(second_batch.iter().any(|e| matches!(
            e,
            Event::Upload {
                kind: UploadKind::CaptureFailed,
                ..
            }
        )));
    }
}

// ── UploadObserver ─────────────────────────────────────────────────────────────

mod upload {
    use super::*;

    fn make(platform: TestPlatformHooks, api: MockApiClient) -> (UploadObserver<MockApiClient>, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let config = UploadConfig {
            batch_interval: Duration::from_secs(60),
        };
        (UploadObserver::new(Box::new(platform), api, config, tx), rx)
    }

    #[test]
    fn upload_when_unauthenticated_is_silently_ignored() {
        let platform = platform_at(1000);
        let api = MockApiClient::new();
        let (mut obs, rx) = make(platform, api.clone());
        obs.on_event(&Event::Upload {
            risk: 0.0,
            kind: UploadKind::Dev {
                title: "ignored".into(),
                details: None,
            },
        })
        .unwrap();
        assert!(drain(&rx).is_empty());
        assert!(api.state().hash_uploads.is_empty());
        assert_eq!(obs.state.pending_hash_events.len(), 0);
    }

    #[test]
    fn ping_when_unauthenticated_does_nothing() {
        let platform = platform_at(1000);
        let api = MockApiClient::new();
        let (mut obs, rx) = make(platform, api.clone());
        obs.on_event(&Event::Ping).unwrap();
        assert!(drain(&rx).is_empty());
        assert!(api.state().hash_uploads.is_empty());
    }

    #[test]
    fn login_event_sets_authenticated_credentials_and_settings() {
        let platform = platform_at(1000);
        let api = MockApiClient::new();
        let (mut obs, _rx) = make(platform, api);
        obs.on_event(&login_event()).unwrap();
        assert!(
            obs.state.settings.is_some(),
            "login should set device settings"
        );
        assert!(
            obs.state.device_credentials.is_some(),
            "login should set device credentials"
        );
        assert_eq!(
            obs.state.post_login_proof_batches_remaining,
            3 // POST_LOGIN_PROOF_BATCH_COUNT
        );
    }

    #[test]
    fn logout_clears_authenticated_state_and_pending_queues() {
        let platform = platform_at(1000);
        let api = MockApiClient::new();
        let (mut obs, _rx) = make(platform, api);
        obs.on_event(&login_event()).unwrap();
        // Push a fake pending event manually
        obs.state.pending_hash_events.push(LogEntry {
            ts: 0,
            risk: Some(0.0),
            event: UploadKind::Dev {
                title: "orphan".into(),
                details: None,
            },
        });
        obs.on_event(&Event::Logout).unwrap();
        assert!(obs.state.settings.is_none(), "logout should clear settings");
        assert!(
            obs.state.device_credentials.is_none(),
            "logout should clear credentials"
        );
        assert!(
            obs.state.pending_hash_events.is_empty(),
            "logout should clear pending hash events"
        );
        assert!(
            obs.state.pending_batch_events.is_empty(),
            "logout should clear pending batch events"
        );
    }

    #[test]
    fn low_risk_upload_goes_through_hash_queue() {
        let platform = platform_at(1000);
        let api = MockApiClient::new();
        let (mut obs, _rx) = make(platform, api.clone());
        obs.on_event(&login_event()).unwrap();
        // Set post_login_proof=0 and a recent batch timestamp to defer batch flush
        obs.state.post_login_proof_batches_remaining = 0;
        obs.state.last_batch_at_ms = Some(1000);

        obs.on_event(&Event::Upload {
            risk: 0.0,
            kind: UploadKind::Dev {
                title: "low-risk".into(),
                details: None,
            },
        })
        .unwrap();
        assert!(
            !api.state().hash_uploads.is_empty(),
            "low-risk Upload should immediately attempt a hash upload"
        );
        assert_eq!(
            api.state().batch_uploads.len(),
            0,
            "batch should not be flushed yet (interval not elapsed)"
        );
    }

    #[test]
    fn high_risk_upload_goes_directly_to_immediate_log() {
        let platform = platform_at(1000);
        let api = MockApiClient::new();
        let (mut obs, _rx) = make(platform, api.clone());
        obs.on_event(&login_event()).unwrap();
        obs.on_event(&Event::Upload {
            risk: 0.9,
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UserStoppedProcess,
            },
        })
        .unwrap();
        assert!(
            !api.state().log_uploads.is_empty(),
            "high-risk Upload should immediately post a direct log upload"
        );
        assert!(
            api.state().hash_uploads.is_empty(),
            "high-risk events should not go through the hash queue"
        );
    }

    #[test]
    fn batch_auto_flushes_during_post_login_proof_window() {
        // post_login_proof=3 means the first 3 batches flush immediately
        let platform = platform_at(1000);
        let api = MockApiClient::new();
        let (mut obs, _rx) = make(platform, api.clone());
        obs.on_event(&login_event()).unwrap();
        assert_eq!(obs.state.post_login_proof_batches_remaining, 3);

        // Each Upload → hash → batch → immediate flush during proof window
        for i in 0..3 {
            obs.on_event(&Event::Upload {
                risk: 0.0,
                kind: UploadKind::Dev {
                    title: format!("event-{i}"),
                    details: None,
                },
            })
            .unwrap();
        }
        assert_eq!(
            api.state().batch_uploads.len(),
            3,
            "all 3 post-login-proof batches should be flushed immediately"
        );
        assert_eq!(obs.state.post_login_proof_batches_remaining, 0);
    }

    #[test]
    fn batch_deferred_after_proof_window_until_interval_elapses() {
        let platform = platform_at(1000);
        let api = MockApiClient::new();
        let (mut obs, _rx) = make(platform.clone(), api.clone());
        obs.on_event(&login_event()).unwrap();
        // Exhaust the proof window
        for i in 0..3 {
            obs.on_event(&Event::Upload {
                risk: 0.0,
                kind: UploadKind::Dev {
                    title: format!("proof-{i}"),
                    details: None,
                },
            })
            .unwrap();
        }
        let after_proof = api.state().batch_uploads.len(); // = 3

        // 4th event: batch deferred (last_batch_at_ms is fresh at t=1000, interval=60s)
        obs.on_event(&Event::Upload {
            risk: 0.0,
            kind: UploadKind::Dev {
                title: "deferred".into(),
                details: None,
            },
        })
        .unwrap();
        assert_eq!(
            api.state().batch_uploads.len(),
            after_proof,
            "batch should be deferred within the 60s interval"
        );

        // Advance past interval and trigger a Ping → batch should flush
        platform.clock.set(62_000);
        obs.on_event(&Event::Ping).unwrap();
        assert!(
            api.state().batch_uploads.len() > after_proof,
            "batch should flush after 60s interval on Ping"
        );
    }

    #[test]
    fn shutdown_flushes_pending_batch_events() {
        let platform = platform_at(1000);
        let api = MockApiClient::new();
        let (mut obs, _rx) = make(platform.clone(), api.clone());
        obs.on_event(&login_event()).unwrap();
        // Exhaust proof window
        for i in 0..3 {
            obs.on_event(&Event::Upload {
                risk: 0.0,
                kind: UploadKind::Dev {
                    title: format!("proof-{i}"),
                    details: None,
                },
            })
            .unwrap();
        }
        let before_shutdown = api.state().batch_uploads.len();
        // 4th event: deferred (batch_at=1000, now=1000, gap=0 < 60s)
        obs.on_event(&Event::Upload {
            risk: 0.0,
            kind: UploadKind::Dev {
                title: "pending".into(),
                details: None,
            },
        })
        .unwrap();
        assert_eq!(api.state().batch_uploads.len(), before_shutdown);

        // Shutdown flushes the deferred batch
        obs.on_event(&Event::ProcessStopped(ProcessStoppedReason::Shutdown))
            .unwrap();
        assert!(
            api.state().batch_uploads.len() > before_shutdown,
            "shutdown should flush the pending batch"
        );
    }

    #[test]
    fn status_request_emits_pending_request_count() {
        let platform = platform_at(1000);
        let api = MockApiClient::new();
        let (mut obs, rx) = make(platform, api);
        obs.on_event(&login_event()).unwrap();
        obs.state.post_login_proof_batches_remaining = 0;
        obs.state.last_batch_at_ms = Some(1000);
        // Push a hash event that won't auto-upload (we're testing the count)
        obs.state.pending_hash_events.push(LogEntry {
            ts: 0,
            risk: Some(0.0),
            event: UploadKind::Dev {
                title: "a".into(),
                details: None,
            },
        });
        obs.state.pending_batch_events.push((500, vec![1, 2, 3]));
        drain(&rx); // clear any login-related events

        obs.on_event(&Event::StatusRequest).unwrap();
        let events = drain(&rx);
        // pending_request_count = hash(1) + immediate(0) + batch(1) = 2
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PartialStatus(PartialStatus::Upload {
                    pending_request_count: 2,
                })
            )),
            "expected PartialStatus::Upload with pending_request_count=2"
        );
    }

    #[test]
    fn device_settings_refreshed_updates_settings() {
        let platform = platform_at(1000);
        let api = MockApiClient::new();
        let (mut obs, _rx) = make(platform, api);
        obs.on_event(&login_event()).unwrap();

        let new_settings = DeviceSettings {
            device_id: "new-device".into(),
            name: "new name".into(),
            platform: "test".into(),
            owner: None,
            partners: Vec::new(),
            hash_base_url: Some("https://hash.example.com".into()),
        };
        obs.on_event(&Event::DeviceSettingsRefreshed {
            settings: new_settings,
        })
        .unwrap();
        assert_eq!(
            obs.state.settings.as_ref().unwrap().device_id,
            "new-device"
        );
        assert_eq!(
            obs.state
                .settings
                .as_ref()
                .unwrap()
                .hash_base_url
                .as_deref(),
            Some("https://hash.example.com")
        );
    }

    #[test]
    fn state_round_trips_through_save_and_load() {
        let platform = platform_at(1000);
        let api = MockApiClient::new();
        let (mut obs, _rx) = make(platform, api);
        obs.on_event(&login_event()).unwrap();
        obs.state.last_batch_at_ms = Some(99_000);
        obs.state.post_login_proof_batches_remaining = 1;
        let saved = obs.save_state().unwrap();

        let (mut obs2, _rx2) = make(platform_at(1000), MockApiClient::new());
        obs2.load_state(saved).unwrap();
        assert_eq!(obs2.state.last_batch_at_ms, Some(99_000));
        assert_eq!(obs2.state.post_login_proof_batches_remaining, 1);
        // Credentials are persisted, so authenticated flag is set
        assert!(
            obs2.state.device_credentials.is_some(),
            "credentials should survive save/load"
        );
    }

    #[test]
    fn ping_with_time_going_backwards_resets_batch_schedule() {
        let platform = platform_at(500);
        let api = MockApiClient::new();
        let (mut obs, _rx) = make(platform.clone(), api);
        obs.on_event(&login_event()).unwrap();
        obs.state.last_batch_at_ms = Some(1000); // "future" timestamp
        obs.state.post_login_proof_batches_remaining = 0;

        // now=500 < last=1000 → schedule resets
        obs.on_event(&Event::Ping).unwrap();
        assert_eq!(
            obs.state.last_batch_at_ms, None,
            "backwards time on Ping should reset the batch schedule"
        );
    }
}

// ── RequestObserver ────────────────────────────────────────────────────────────

mod request_handler {
    use super::*;

    #[test]
    fn no_clients_all_events_return_ok() {
        let mut obs = RequestObserver::new();
        let events: Vec<Event> = vec![
            Event::Ping,
            Event::ProcessStarted,
            Event::ComputerSuspended,
            Event::ComputerResumed,
            Event::StatusRequest,
            Event::UserSessionLogin,
            Event::UserSessionLogout,
            Event::Logout,
            Event::CaptureFailed,
            Event::LogoutRequested,
            Event::Upload {
                risk: 0.5,
                kind: UploadKind::CaptureFailed,
            },
        ];
        for event in events {
            obs.on_event(&event)
                .expect("RequestObserver::on_event must not fail");
        }
    }

    #[test]
    fn save_and_load_state_are_no_ops() {
        let mut obs = RequestObserver::new();
        let state = obs.save_state().unwrap();
        assert!(state.is_null());
        obs.load_state(serde_json::Value::Null).unwrap();
    }

    // IPC forwarding / filtering tests require a real socket connection
    // and are platform-specific.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    mod unix_forwarding {
        use std::io::{BufRead, BufReader};
        use std::os::unix::net::UnixStream;

        use super::*;
        use virtue_core::ipc::{IpcListener, connect};

        fn unique_sock_path(suffix: &str) -> std::path::PathBuf {
            std::env::temp_dir().join(format!(
                "virtue-req-test-{}-{}.sock",
                std::process::id(),
                suffix,
            ))
        }

        /// Spawns a background thread that reads newline-delimited JSON events
        /// from a `UnixStream` and sends them over an `mpsc` channel.
        fn spawn_reader(stream: UnixStream) -> Receiver<Event> {
            let (proxy_tx, proxy_rx) = mpsc::channel();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            if let Ok(ev) = serde_json::from_str::<Event>(line.trim()) {
                                proxy_tx.send(ev).ok();
                            }
                        }
                    }
                }
            });
            proxy_rx
        }

        /// Creates an `IpcSender` (daemon side) and a proxy `Receiver<Event>`
        /// (controller side) that collect forwarded events for inspection.
        fn make_ipc_pair(suffix: &str) -> (virtue_core::ipc::IpcSender, Receiver<Event>) {
            let sock = unique_sock_path(suffix);
            let listener = IpcListener::bind(&sock).unwrap();

            let sock2 = sock.clone();
            let connect_handle = std::thread::spawn(move || {
                // connect() returns (IpcSender, IpcReceiver); we only need the stream half
                let stream = UnixStream::connect(&sock2).unwrap();
                stream
            });

            let (daemon_sender, _daemon_receiver) = listener.blocking_accept().unwrap();
            let ctrl_stream = connect_handle.join().unwrap();
            let proxy_rx = spawn_reader(ctrl_stream);

            let _ = std::fs::remove_file(&sock);
            (daemon_sender, proxy_rx)
        }

        fn wait_for_events(rx: &Receiver<Event>, count: usize) -> Vec<Event> {
            let deadline =
                std::time::Instant::now() + std::time::Duration::from_millis(500);
            let mut collected = Vec::new();
            while collected.len() < count && std::time::Instant::now() < deadline {
                if let Ok(ev) = rx.try_recv() {
                    collected.push(ev);
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
            collected
        }

        #[test]
        fn upload_ping_login_and_partial_status_are_not_forwarded() {
            let (daemon_sender, proxy_rx) = make_ipc_pair("blocked");
            let mut obs = RequestObserver::new();
            obs.add_client(daemon_sender);

            obs.on_event(&Event::Ping).unwrap();
            obs.on_event(&Event::Upload {
                risk: 0.0,
                kind: UploadKind::CaptureFailed,
            })
            .unwrap();
            obs.on_event(&Event::PartialStatus(PartialStatus::Upload {
                pending_request_count: 0,
            }))
            .unwrap();
            obs.on_event(&login_event()).unwrap();

            std::thread::sleep(std::time::Duration::from_millis(50));
            let received: Vec<_> = std::iter::from_fn(|| proxy_rx.try_recv().ok()).collect();
            assert!(
                received.is_empty(),
                "blocked events (Upload, Ping, Login, PartialStatus) must not be forwarded: {received:?}"
            );
        }

        #[test]
        fn allowed_events_are_forwarded_to_clients() {
            let (daemon_sender, proxy_rx) = make_ipc_pair("allowed");
            let mut obs = RequestObserver::new();
            obs.add_client(daemon_sender);

            obs.on_event(&Event::UserSessionLogin).unwrap();
            obs.on_event(&Event::ComputerSuspended).unwrap();
            obs.on_event(&Event::Logout).unwrap();

            let received = wait_for_events(&proxy_rx, 3);
            assert_eq!(received.len(), 3, "all 3 allowed events should be forwarded");
            assert!(matches!(received[0], Event::UserSessionLogin));
            assert!(matches!(received[1], Event::ComputerSuspended));
            assert!(matches!(received[2], Event::Logout));
        }

        #[test]
        fn disconnected_client_is_removed_on_next_send() {
            let sock = unique_sock_path("drop");
            let listener = IpcListener::bind(&sock).unwrap();

            let sock2 = sock.clone();
            let connect_handle = std::thread::spawn(move || connect(&sock2).unwrap());
            let (daemon_sender, _daemon_receiver) = listener.blocking_accept().unwrap();
            let (_ctrl_sender, ctrl_receiver_ipc) = connect_handle.join().unwrap();

            let mut obs = RequestObserver::new();
            obs.add_client(daemon_sender);

            // Verify a send works while client is connected
            obs.on_event(&Event::UserSessionLogin).unwrap();

            // Drop the client's read end — next write to the socket will fail
            drop(ctrl_receiver_ipc);
            std::thread::sleep(std::time::Duration::from_millis(50));

            // This send should silently drop the dead client
            obs.on_event(&Event::ComputerSuspended).unwrap();

            // Next allowed event should not cause a panic (no clients left)
            obs.on_event(&Event::ComputerResumed).unwrap();

            let _ = std::fs::remove_file(&sock);
        }
    }
}
