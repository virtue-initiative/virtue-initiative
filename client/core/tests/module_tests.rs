//! Module-level behavioral tests.
//!
//! These tests exercise each observer module in isolation through the new
//! EventBus API. Each test builds a single-module bus, sends typed events,
//! and inspects emitted events captured via Arc<Mutex<Vec<_>>> subscriptions.
//!
//! Run with:
//!   cargo test -p virtue-core --features testing --test module_tests

use std::sync::{Arc, Mutex};

use virtue_core::events::bus::EventBus;
use virtue_core::events::bus::StateType;
use virtue_core::events::types::{
    CaptureFailed, ComputerResumed, ComputerSuspended, Login, Logout, PartialStatus, Ping,
    ProcessStarted, ProcessStopped, StatusRequest, StatusResponse, Upload, UserSessionLogin,
    UserSessionLogout,
};
use virtue_core::model::{
    AlertReason, BatchRecipient, DeviceCredentials, DeviceSettings, LifecycleKind, LogEntry,
    ProcessStoppedReason, UploadKind,
};
use virtue_core::module::capture_availability::CaptureAvailabilityModule;
use virtue_core::module::lifecycle::{LifecycleModule, LifecycleStatus};
use virtue_core::module::screenshot::ScreenshotModule;
use virtue_core::module::status::StatusModule;
use virtue_core::module::upload::UploadModule;
use virtue_core::testing::{MockApiClient, TestPlatformHooks};

// ── Helpers ────────────────────────────────────────────────────────────────────

fn valid_settings() -> DeviceSettings {
    DeviceSettings {
        device_id: "test-device".into(),
        name: "test device".into(),
        platform: "test".into(),
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

fn login_event() -> Login {
    Login {
        credentials: valid_credentials(),
        settings: valid_settings(),
    }
}

// ── LifecycleModule ────────────────────────────────────────────────────────────

mod lifecycle {
    use super::*;

    fn make(
        ts: i64,
    ) -> (
        EventBus,
        Arc<Mutex<Vec<Upload>>>,
        Arc<Mutex<Vec<PartialStatus>>>,
    ) {
        let platform = TestPlatformHooks::new();
        platform.clock.set(ts);
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
        (bus, uploads, partials)
    }

    #[test]
    fn status_request_emits_lifecycle_partial_status() {
        let (mut bus, _, partials) = make(1_000);
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
        let (mut bus, uploads, _) = make(1_000);
        bus.send(ProcessStarted).unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::Lifecycle {
                    kind: LifecycleKind::ProcessStarted,
                }
            )),
            "expected ProcessStarted lifecycle upload"
        );
    }

    #[test]
    fn process_stopped_shutdown_emits_upload() {
        let (mut bus, uploads, _) = make(2_000);
        bus.send(ProcessStopped(ProcessStoppedReason::Shutdown))
            .unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::Lifecycle {
                    kind: LifecycleKind::ProcessStoppedShutdown,
                }
            )),
            "expected ProcessStoppedShutdown lifecycle upload"
        );
    }

    #[test]
    fn process_stopped_user_emits_upload_and_high_risk_alert() {
        let (mut bus, uploads, _) = make(4_000);
        bus.send(ProcessStopped(ProcessStoppedReason::User))
            .unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::Lifecycle {
                    kind: LifecycleKind::ProcessStoppedUser,
                }
            )),
            "expected ProcessStoppedUser lifecycle upload"
        );
        let alert = u.iter().find(|e| {
            matches!(
                e.kind,
                UploadKind::LifecycleAlert {
                    reason: AlertReason::UserStoppedProcess,
                }
            )
        });
        assert!(alert.is_some(), "expected UserStoppedProcess alert");
        assert!(alert.unwrap().risk >= 0.9, "alert should be high risk");
    }

    #[test]
    fn computer_suspended_emits_upload() {
        let (mut bus, uploads, _) = make(5_000);
        bus.send(ComputerSuspended).unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::Lifecycle {
                    kind: LifecycleKind::ComputerSuspended,
                }
            )),
            "expected ComputerSuspended lifecycle upload"
        );
    }

    #[test]
    fn computer_resumed_after_suspend_emits_upload() {
        let (mut bus, uploads, _) = make(6_000);
        bus.send(ComputerSuspended).unwrap();
        bus.iter().unwrap();
        uploads.lock().unwrap().clear();
        bus.send(ComputerResumed).unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::Lifecycle {
                    kind: LifecycleKind::ComputerResumed,
                }
            )),
            "expected ComputerResumed lifecycle upload"
        );
    }

    #[test]
    fn fourth_ping_while_suspended_triggers_missing_resume_alert() {
        let platform = TestPlatformHooks::new();
        platform.clock.set(1_000);
        let mut module = LifecycleModule::new(Box::new(platform));
        // Pre-set state to suspended with 3 pings already counted.
        module.state.status = LifecycleStatus::Suspended;
        module.state.pings_while_suspended = 3;

        let uploads: Arc<Mutex<Vec<Upload>>> = Arc::new(Mutex::new(Vec::new()));
        let u = Arc::clone(&uploads);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &Upload| {
            u.lock().unwrap().push(ev.clone());
            Ok(())
        });

        bus.send(Ping).unwrap();
        bus.iter().unwrap();

        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::LifecycleAlert {
                    reason: AlertReason::MissingResume,
                }
            )),
            "expected MissingResume alert on 4th ping while suspended"
        );
    }

    #[test]
    fn session_login_emits_lifecycle_upload() {
        let (mut bus, uploads, _) = make(1_000);
        bus.send(UserSessionLogin).unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::Lifecycle {
                    kind: LifecycleKind::Login,
                }
            )),
            "expected Login lifecycle upload"
        );
    }

    #[test]
    fn session_logout_emits_high_risk_lifecycle_upload() {
        let (mut bus, uploads, _) = make(1_000);
        bus.send(UserSessionLogout).unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        let upload = u.iter().find(|e| {
            matches!(
                e.kind,
                UploadKind::Lifecycle {
                    kind: LifecycleKind::Logout,
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
        let platform = TestPlatformHooks::new();
        platform.clock.set(100_000);
        let mut module = LifecycleModule::new(Box::new(platform));
        module.state.last_login = 0;
        module.state.last_ping = 1_000;
        module.state.last_running_started = 1_000;

        let uploads: Arc<Mutex<Vec<Upload>>> = Arc::new(Mutex::new(Vec::new()));
        let u = Arc::clone(&uploads);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &Upload| {
            u.lock().unwrap().push(ev.clone());
            Ok(())
        });

        bus.send(Ping).unwrap();
        bus.iter().unwrap();

        let u = uploads.lock().unwrap();
        let alert = u.iter().find(|e| {
            matches!(
                e.kind,
                UploadKind::LifecycleAlert {
                    reason: AlertReason::PingGapWhileRunning,
                }
            )
        });
        assert!(alert.is_some(), "expected PingGapWhileRunning alert");
        assert!(
            alert.unwrap().risk >= 0.9,
            "ping gap alert should be high risk"
        );
    }

    #[test]
    fn ping_within_login_grace_period_does_not_emit_alert() {
        let platform = TestPlatformHooks::new();
        platform.clock.set(30_000);
        let mut module = LifecycleModule::new(Box::new(platform));
        module.state.last_login = 20_000;
        module.state.last_ping = 1_000;
        module.state.last_running_started = 1_000;

        let uploads: Arc<Mutex<Vec<Upload>>> = Arc::new(Mutex::new(Vec::new()));
        let u = Arc::clone(&uploads);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &Upload| {
            u.lock().unwrap().push(ev.clone());
            Ok(())
        });

        bus.send(Ping).unwrap();
        bus.iter().unwrap();

        let u = uploads.lock().unwrap();
        assert!(
            !u.iter().any(|e| matches!(
                e.kind,
                UploadKind::LifecycleAlert {
                    reason: AlertReason::PingGapWhileRunning,
                }
            )),
            "ping gap alert should be suppressed within 60 s login grace window"
        );
    }

    #[test]
    fn process_killed_before_shutdown_emits_alert() {
        let platform = TestPlatformHooks::new();
        platform.clock.set(20_000);
        let mut module = LifecycleModule::new(Box::new(platform));
        module.state.last_process_stopped_other = 1_000;
        module.state.last_process_stopped_shutdown = 12_000;

        let uploads: Arc<Mutex<Vec<Upload>>> = Arc::new(Mutex::new(Vec::new()));
        let u = Arc::clone(&uploads);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &Upload| {
            u.lock().unwrap().push(ev.clone());
            Ok(())
        });

        bus.send(ProcessStarted).unwrap();
        bus.iter().unwrap();

        let u = uploads.lock().unwrap();
        assert!(
            u.iter().any(|e| matches!(
                e.kind,
                UploadKind::LifecycleAlert {
                    reason: AlertReason::ProcessKilledBeforeShutdown,
                }
            )),
            "expected ProcessKilledBeforeShutdown alert"
        );
    }

    #[test]
    fn state_round_trips_through_save_and_load() {
        let platform = TestPlatformHooks::new();
        platform.clock.set(1_000);
        let mut module = LifecycleModule::new(Box::new(platform));
        module.state.last_login = 42_000;
        module.state.last_ping = 99_000;
        module.state.pings_while_suspended = 2;
        module.state.last_running_started = 55_000;

        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        let saved = bus.save().unwrap();

        // Load into a new bus and verify state was restored.
        let platform2 = TestPlatformHooks::new();
        let module2 = LifecycleModule::new(Box::new(platform2));
        let mut bus2 = EventBus::new(vec![Box::new(module2)], saved).unwrap();

        let m = bus2
            .observer_mut("lifecycle")
            .unwrap()
            .as_any_mut()
            .downcast_mut::<LifecycleModule>()
            .unwrap();
        assert_eq!(m.state.last_login, 42_000);
        assert_eq!(m.state.last_ping, 99_000);
        assert_eq!(m.state.pings_while_suspended, 2);
        assert_eq!(m.state.last_running_started, 55_000);
    }
}

// ── ScreenshotModule ───────────────────────────────────────────────────────────

mod screenshot {
    use super::*;

    fn make(ts: i64) -> (EventBus, Arc<Mutex<Vec<Upload>>>, TestPlatformHooks) {
        let platform = TestPlatformHooks::new();
        platform.clock.set(ts);
        let platform_clone = platform.clone();
        let module = ScreenshotModule::new(Box::new(platform), 60_000);
        let uploads: Arc<Mutex<Vec<Upload>>> = Arc::new(Mutex::new(Vec::new()));
        let u = Arc::clone(&uploads);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &Upload| {
            u.lock().unwrap().push(ev.clone());
            Ok(())
        });
        (bus, uploads, platform_clone)
    }

    #[test]
    fn ping_when_unauthenticated_does_nothing() {
        let (mut bus, uploads, platform) = make(1_000);
        bus.send(Ping).unwrap();
        bus.iter().unwrap();
        assert!(uploads.lock().unwrap().is_empty());
        assert_eq!(platform.take_call_count(), 0);
    }

    #[test]
    fn login_then_ping_takes_screenshot() {
        let (mut bus, uploads, platform) = make(1_000);
        bus.send(Login {
            credentials: valid_credentials(),
            settings: valid_settings(),
        })
        .unwrap();
        bus.iter().unwrap();
        uploads.lock().unwrap().clear();

        bus.send(Ping).unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter()
                .any(|e| matches!(e.kind, UploadKind::Screenshot { .. })),
            "expected Screenshot upload after first ping post-login"
        );
        assert_eq!(platform.take_call_count(), 1);
    }

    #[test]
    fn screenshot_not_retaken_before_interval() {
        let platform = TestPlatformHooks::new();
        platform.clock.set(30_000);
        let mut module = ScreenshotModule::new(Box::new(platform.clone()), 60_000);
        module.state.authenticated = true;
        module.state.last_screenshot_at_ms = Some(0);

        let uploads: Arc<Mutex<Vec<Upload>>> = Arc::new(Mutex::new(Vec::new()));
        let u = Arc::clone(&uploads);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &Upload| {
            u.lock().unwrap().push(ev.clone());
            Ok(())
        });

        bus.send(Ping).unwrap();
        bus.iter().unwrap();
        assert_eq!(platform.take_call_count(), 0);
    }

    #[test]
    fn screenshot_retaken_after_interval() {
        let platform = TestPlatformHooks::new();
        platform.clock.set(61_000);
        let mut module = ScreenshotModule::new(Box::new(platform.clone()), 60_000);
        module.state.authenticated = true;
        module.state.last_screenshot_at_ms = Some(0);

        let uploads: Arc<Mutex<Vec<Upload>>> = Arc::new(Mutex::new(Vec::new()));
        let u = Arc::clone(&uploads);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &Upload| {
            u.lock().unwrap().push(ev.clone());
            Ok(())
        });

        bus.send(Ping).unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter()
                .any(|e| matches!(e.kind, UploadKind::Screenshot { .. })),
            "expected screenshot after interval elapsed"
        );
        assert_eq!(platform.take_call_count(), 1);
    }

    #[test]
    fn logout_clears_authenticated_and_schedule() {
        let platform = TestPlatformHooks::new();
        let mut module = ScreenshotModule::new(Box::new(platform), 60_000);
        module.state.authenticated = true;
        module.state.last_screenshot_at_ms = Some(500);

        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.send(Logout).unwrap();
        bus.iter().unwrap();

        let m = bus
            .observer_mut("screenshot")
            .unwrap()
            .as_any_mut()
            .downcast_mut::<ScreenshotModule>()
            .unwrap();
        assert!(!m.state.authenticated);
        assert_eq!(m.state.last_screenshot_at_ms, None);
    }
}

// ── StatusModule ───────────────────────────────────────────────────────────────

mod status {
    use super::*;

    fn make(expected_count: usize) -> (EventBus, Arc<Mutex<Vec<StatusResponse>>>) {
        let module = StatusModule::new(expected_count);
        let responses: Arc<Mutex<Vec<StatusResponse>>> = Arc::new(Mutex::new(Vec::new()));
        let r = Arc::clone(&responses);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &StatusResponse| {
            r.lock().unwrap().push(ev.clone());
            Ok(())
        });
        (bus, responses)
    }

    #[test]
    fn one_partial_with_expected_one_triggers_response() {
        let (mut bus, responses) = make(1);
        bus.send(StatusRequest).unwrap();
        bus.send(PartialStatus::Auth {
            is_authenticated: true,
            device_id: Some("dev".into()),
        })
        .unwrap();
        bus.iter().unwrap();
        assert_eq!(
            responses.lock().unwrap().len(),
            1,
            "expected StatusResponse"
        );
    }

    #[test]
    fn response_only_after_all_expected_fragments_received() {
        let (mut bus, responses) = make(3);
        bus.send(StatusRequest).unwrap();
        bus.send(PartialStatus::Auth {
            is_authenticated: false,
            device_id: None,
        })
        .unwrap();
        bus.iter().unwrap();
        assert!(
            responses.lock().unwrap().is_empty(),
            "should not respond after 1 of 3"
        );

        bus.send(PartialStatus::Lifecycle {
            is_running: true,
            last_loop_at_ms: Some(1_000),
        })
        .unwrap();
        bus.iter().unwrap();
        assert!(
            responses.lock().unwrap().is_empty(),
            "should not respond after 2 of 3"
        );

        bus.send(PartialStatus::Upload {
            pending_request_count: 7,
        })
        .unwrap();
        bus.iter().unwrap();
        let r = responses.lock().unwrap();
        assert_eq!(r.len(), 1);
        assert!(!r[0].status.is_authenticated);
        assert!(r[0].status.is_running);
        assert_eq!(r[0].status.last_loop_at_ms, Some(1_000));
        assert_eq!(r[0].status.pending_request_count, 7);
    }

    #[test]
    fn new_status_request_resets_accumulated_state() {
        let (mut bus, responses) = make(1);
        bus.send(StatusRequest).unwrap();
        bus.send(PartialStatus::Auth {
            is_authenticated: true,
            device_id: Some("dev1".into()),
        })
        .unwrap();
        bus.iter().unwrap();
        assert_eq!(responses.lock().unwrap().len(), 1);
        responses.lock().unwrap().clear();

        bus.send(StatusRequest).unwrap();
        bus.send(PartialStatus::Auth {
            is_authenticated: false,
            device_id: None,
        })
        .unwrap();
        bus.iter().unwrap();
        let r = responses.lock().unwrap();
        assert_eq!(r.len(), 1);
        assert!(!r[0].status.is_authenticated);
    }
}

// ── CaptureAvailabilityModule ──────────────────────────────────────────────────

mod capture_availability {
    use super::*;

    fn make(ts: i64) -> (EventBus, Arc<Mutex<Vec<Upload>>>) {
        let platform = TestPlatformHooks::new();
        platform.clock.set(ts);
        let module = CaptureAvailabilityModule::new(Box::new(platform));
        let uploads: Arc<Mutex<Vec<Upload>>> = Arc::new(Mutex::new(Vec::new()));
        let u = Arc::clone(&uploads);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &Upload| {
            u.lock().unwrap().push(ev.clone());
            Ok(())
        });
        (bus, uploads)
    }

    #[test]
    fn four_failures_below_threshold_no_upload() {
        let (mut bus, uploads) = make(1_000);
        for _ in 0..4 {
            bus.send(CaptureFailed).unwrap();
        }
        bus.iter().unwrap();
        assert!(
            uploads.lock().unwrap().is_empty(),
            "4 failures should not trigger an upload"
        );
    }

    #[test]
    fn fifth_failure_triggers_capture_failed_upload() {
        let (mut bus, uploads) = make(1_000);
        for _ in 0..5 {
            bus.send(CaptureFailed).unwrap();
        }
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter()
                .any(|e| matches!(e.kind, UploadKind::CaptureFailed)),
            "5 failures should trigger a CaptureFailed upload"
        );
    }
}

// ── UploadModule ───────────────────────────────────────────────────────────────

mod upload {
    use super::*;

    // Upload module tests are integration-tested via Scenario (see scenarios.rs).
    #[test]
    fn upload_when_unauthenticated_is_silently_ignored() {
        let api = MockApiClient::new();
        let platform = TestPlatformHooks::new();
        let module = UploadModule::new(Box::new(platform), api.clone(), 60_000);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.send(Upload {
            risk: 0.0,
            kind: UploadKind::Dev {
                title: "ignored".into(),
                details: None,
            },
        })
        .unwrap();
        bus.iter().unwrap();
        assert!(api.state().hash_uploads.is_empty());
        let m = bus
            .observer_mut("upload")
            .unwrap()
            .as_any_mut()
            .downcast_mut::<UploadModule<MockApiClient>>()
            .unwrap();
        assert_eq!(m.state.pending_hash_events.len(), 0);
    }

    #[test]
    fn login_sets_authenticated_credentials_and_settings() {
        let api = MockApiClient::new();
        let platform = TestPlatformHooks::new();
        let module = UploadModule::new(Box::new(platform), api, 60_000);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.send(login_event()).unwrap();
        bus.iter().unwrap();
        let m = bus
            .observer_mut("upload")
            .unwrap()
            .as_any_mut()
            .downcast_mut::<UploadModule<MockApiClient>>()
            .unwrap();
        assert!(
            m.state.settings.is_some(),
            "login should set device settings"
        );
        assert!(
            m.state.device_credentials.is_some(),
            "login should set credentials"
        );
        assert_eq!(m.state.post_login_proof_batches_remaining, 3);
    }

    #[test]
    fn logout_clears_authenticated_state() {
        let api = MockApiClient::new();
        let platform = TestPlatformHooks::new();
        let module = UploadModule::new(Box::new(platform), api, 60_000);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.send(login_event()).unwrap();
        bus.iter().unwrap();
        bus.send(Logout).unwrap();
        bus.iter().unwrap();
        let m = bus
            .observer_mut("upload")
            .unwrap()
            .as_any_mut()
            .downcast_mut::<UploadModule<MockApiClient>>()
            .unwrap();
        assert!(m.state.settings.is_none(), "logout should clear settings");
        assert!(
            m.state.device_credentials.is_none(),
            "logout should clear credentials"
        );
        assert!(
            m.state.pending_hash_events.is_empty(),
            "logout should clear pending events"
        );
        assert!(
            m.state.pending_batch_events.is_empty(),
            "logout should clear batch queue"
        );
    }

    #[test]
    fn status_request_emits_pending_request_count() {
        let api = MockApiClient::new();
        let platform = TestPlatformHooks::new();
        platform.clock.set(1_000);
        let module = UploadModule::new(Box::new(platform), api, 60_000);

        let partials: Arc<Mutex<Vec<PartialStatus>>> = Arc::new(Mutex::new(Vec::new()));
        let p = Arc::clone(&partials);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &PartialStatus| {
            p.lock().unwrap().push(ev.clone());
            Ok(())
        });

        // Manually insert pending items
        {
            let m = bus
                .observer_mut("upload")
                .unwrap()
                .as_any_mut()
                .downcast_mut::<UploadModule<MockApiClient>>()
                .unwrap();
            m.authenticated = true;
            m.state.device_credentials = Some(valid_credentials());
            m.state.post_login_proof_batches_remaining = 0;
            m.state.last_batch_at_ms = Some(1_000);
            m.state.pending_hash_events.push(LogEntry {
                ts: 0,
                risk: Some(0.0),
                event: UploadKind::Dev {
                    title: "a".into(),
                    details: None,
                },
            });
            m.state.pending_batch_events.push((500, vec![1, 2, 3]));
        }

        bus.send(StatusRequest).unwrap();
        bus.iter().unwrap();

        let p = partials.lock().unwrap();
        assert!(
            p.iter().any(|s| matches!(
                s,
                PartialStatus::Upload {
                    pending_request_count: 2,
                }
            )),
            "expected PartialStatus::Upload with pending_request_count=2"
        );
    }
}
