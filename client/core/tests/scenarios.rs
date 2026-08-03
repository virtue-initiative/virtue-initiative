//! Behavioral scenario tests using the `Scenario` DSL.
//!
//! Each scenario uses `virtue_core::testing::Scenario` to drive the service
//! through a sequence of events without real HTTP, real screenshots, or
//! wall-clock time.
//!
//! Run with:
//!   cargo test -p virtue-core --features testing --test scenarios

use virtue_core::module::lifecycle::LifecycleObserverState;
use virtue_core::testing::Scenario;
use virtue_core::{CaptureFailed, LogoutRequested, Upload, UploadKind, UserStopRequested};

// ── Basic unauthenticated loop ────────────────────────────────────────────────

#[test]
fn fresh_unauthenticated_service_loops_cleanly_with_no_uploads() {
    let mut scenario = Scenario::new();

    scenario
        .assert_is_running(true)
        .assert_is_authenticated(false);

    scenario
        .at_t(0)
        .loop_iteration()
        .at_t(60_000)
        .loop_iteration()
        .at_t(120_000)
        .loop_iteration();

    scenario
        .assert_is_running(true)
        .assert_is_authenticated(false)
        .assert_batch_upload_count(0)
        .assert_notify_count(0)
        .assert_errors_log_empty();
}

// ── ScreenshotModule ───────────────────────────────────────────────────────────

#[test]
fn screenshot_taken_immediately_on_first_loop() {
    let mut scenario = Scenario::authenticated();
    scenario.at_t(0).loop_iteration();
    // Screenshot flows through batch; at least one upload expected.
    let has_upload = {
        let s = scenario.api.state();
        !s.batch_uploads.is_empty() || !s.hash_uploads.is_empty()
    };
    assert!(
        has_upload,
        "expected at least one upload after first authenticated loop"
    );
}

#[test]
fn screenshot_not_retaken_before_interval_elapses() {
    let mut scenario = Scenario::authenticated();
    scenario.set_last_screenshot_at_ms(Some(0));
    let uploads_before = {
        let s = scenario.api.state();
        s.batch_uploads.len() + s.hash_uploads.len()
    };

    scenario.at_t(30_000).loop_iteration();

    let uploads_after = {
        let s = scenario.api.state();
        s.batch_uploads.len() + s.hash_uploads.len()
    };
    assert_eq!(
        uploads_after, uploads_before,
        "no new uploads expected before 60 s screenshot interval"
    );
}

#[test]
fn screenshot_retaken_after_interval() {
    let mut scenario = Scenario::authenticated();
    scenario.set_last_screenshot_at_ms(Some(0));
    scenario.set_last_batch_at_ms(Some(0));

    scenario.at_t(60_001).loop_iteration();

    let has_upload = {
        let s = scenario.api.state();
        !s.batch_uploads.is_empty() || !s.hash_uploads.is_empty()
    };
    assert!(has_upload, "expected screenshot upload after 60 s interval");
}

// ── LifecycleModule ───────────────────────────────────────────────────────────

#[test]
fn login_poll_emits_lifecycle_upload() {
    let mut scenario = Scenario::authenticated();
    scenario.platform.set_last_login(Some(500));
    scenario.at_t(1_000).loop_iteration();
    assert!(
        !scenario.api.state().hash_uploads.is_empty(),
        "expected at least one hash upload from the SystemLogin lifecycle event"
    );
}

#[test]
fn user_stop_emits_high_risk_upload() {
    let mut scenario = Scenario::authenticated();
    scenario.queue(UserStopRequested {
        source: "test".into(),
    });
    scenario.at_t(0).loop_iteration();
    assert!(
        !scenario.api.state().notify_calls.is_empty(),
        "expected an immediate notification for UserStop alert"
    );
}

#[test]
fn unexpected_gap_batches_alert_without_email() {
    let mut scenario = Scenario::authenticated();
    // First loop establishes a baseline heartbeat sample.
    scenario.at_t(0).loop_iteration();
    // Suppress a second screenshot so the only new upload traffic is the alert itself.
    scenario.set_last_screenshot_at_ms(Some(121_000));
    let hashes_before = scenario.api.state().hash_uploads.len();
    // A single 121s gap exceeds the sliding-window budget (120s) in one shot, so the
    // UnexpectedGap alert fires — but it's high-risk, not immediate.
    scenario.at_t(121_000).loop_iteration();
    let state = scenario.api.state();
    assert!(
        state.notify_calls.is_empty(),
        "unexpected gap must not trigger an immediate (emailed) notification"
    );
    assert!(
        state.hash_uploads.len() > hashes_before,
        "unexpected gap alert should still be recorded via the batched hash path"
    );
}

#[test]
fn suspend_divergence_suppresses_unexpected_gap_alert() {
    let mut scenario = Scenario::authenticated();
    scenario.at_t(1_000).loop_iteration();
    // Simulate an 11s suspend: monotonic only advances 1s while boot/wall-clock
    // advance 12s — the awake gap (1s) is well under threshold, so no alert.
    scenario.platform.set_boot_clock_ms(12_000);
    scenario.platform.set_monotonic_clock_ms(2_000);
    scenario.at_t(12_000).loop_iteration();
    assert_eq!(
        scenario.api.state().notify_calls.len(),
        0,
        "expected no immediate notification when a suspend explains the gap"
    );
}

// ── CaptureAvailabilityModule ─────────────────────────────────────────────────

#[test]
fn four_capture_failures_below_threshold_no_upload() {
    let mut scenario = Scenario::authenticated();
    scenario.set_last_screenshot_at_ms(Some(0));
    for _ in 0..4 {
        scenario.queue(CaptureFailed);
    }
    scenario.at_t(30_000).loop_iteration();
    let state = scenario.api.state();
    assert_eq!(
        state.hash_uploads.len(),
        0,
        "4 failures < threshold, no hash upload"
    );
    assert_eq!(
        state.batch_uploads.len(),
        0,
        "4 failures < threshold, no batch upload"
    );
    assert_eq!(
        state.notify_calls.len(),
        0,
        "4 failures < threshold, no notification"
    );
}

#[test]
fn five_capture_failures_triggers_upload() {
    let mut scenario = Scenario::authenticated();
    scenario.set_last_screenshot_at_ms(Some(0));
    scenario.set_last_batch_at_ms(Some(0));
    for _ in 0..5 {
        scenario.queue(CaptureFailed);
    }
    scenario.at_t(30_000).loop_iteration();
    assert!(
        !scenario.api.state().hash_uploads.is_empty(),
        "5 failures == threshold should trigger Upload → hash upload"
    );
}

// ── UploadModule ──────────────────────────────────────────────────────────────

#[test]
fn low_risk_upload_queued_until_batch_interval() {
    let mut scenario = Scenario::authenticated();
    scenario.set_last_batch_at_ms(Some(0));
    scenario.queue(Upload {
        risk: 0.0,
        kind: UploadKind::Dev {
            title: "test-event".into(),
            details: None,
        },
    });
    scenario.at_t(30_000).loop_iteration();
    assert!(
        !scenario.api.state().hash_uploads.is_empty(),
        "hash should upload immediately for low-risk events"
    );
    assert_eq!(
        scenario.api.state().batch_uploads.len(),
        0,
        "batch should not flush before the 60 s interval"
    );
}

#[test]
fn batch_flushed_after_interval() {
    let mut scenario = Scenario::authenticated();
    scenario.set_last_batch_at_ms(Some(0));
    scenario.queue(Upload {
        risk: 0.0,
        kind: UploadKind::Dev {
            title: "test-event".into(),
            details: None,
        },
    });
    scenario.at_t(30_000).loop_iteration();
    assert_eq!(scenario.api.state().batch_uploads.len(), 0);
    scenario.at_t(60_000).loop_iteration();
    assert!(
        !scenario.api.state().batch_uploads.is_empty(),
        "batch should flush after 60 s interval"
    );
}

#[test]
fn logout_clears_pending_state() {
    let mut scenario = Scenario::authenticated();
    scenario.set_last_batch_at_ms(Some(0));
    scenario.queue(Upload {
        risk: 0.0,
        kind: UploadKind::Dev {
            title: "pending-event".into(),
            details: None,
        },
    });
    scenario.at_t(30_000).loop_iteration();
    assert_eq!(
        scenario.api.state().batch_uploads.len(),
        0,
        "precondition: batch not yet flushed"
    );
    scenario.queue(LogoutRequested);
    scenario.at_t(30_000).loop_iteration();
    assert_eq!(
        scenario.api.state().batch_uploads.len(),
        0,
        "pending batch events should be discarded on logout"
    );
}

// ── LifecycleModule alert paths ───────────────────────────────────────────────

#[test]
fn unexpected_start_after_long_gap_since_login_emits_alert() {
    let mut scenario = Scenario::authenticated();
    scenario.platform.set_last_login(Some(0));
    scenario.platform.set_boot_clock_ms(241_000);
    scenario.platform.set_monotonic_clock_ms(241_000);
    scenario.at_t(241_000).loop_iteration();
    // UnexpectedStart is high-risk but batched (no immediate email).
    let state = scenario.api.state();
    assert!(
        state.notify_calls.is_empty(),
        "unexpected start must not trigger an immediate (emailed) notification"
    );
    assert!(
        !state.hash_uploads.is_empty() || !state.batch_uploads.is_empty(),
        "unexpected start alert should still be recorded via the batched path"
    );
}

#[test]
fn unexpected_stop_emits_alert_when_logout_arrives_after_last_sample() {
    let mut scenario = Scenario::authenticated();
    scenario.at_t(1_000).loop_iteration();
    scenario.platform.set_last_logout(Some(62_000));
    scenario.at_t(62_000).loop_iteration();
    assert!(
        !scenario.api.state().hash_uploads.is_empty(),
        "expected an UnexpectedStop hash upload"
    );
}

#[test]
fn lifecycle_observer_state_can_be_seeded_directly() {
    let mut scenario = Scenario::authenticated();
    scenario.set_lifecycle_observer_state(LifecycleObserverState {
        login_id: 5,
        last_login_utc_ms: 1_000,
        ..Default::default()
    });
    scenario.at_t(1_000).loop_iteration();
    assert!(
        scenario.api.state().notify_calls.is_empty(),
        "seeding lifecycle state alone must not trigger any alert"
    );
}

// ── State persistence ─────────────────────────────────────────────────────────

#[test]
fn screenshot_state_survives_restart() {
    // Create first service, backdate screenshot so the schedule shows "just taken".
    let mut scenario1 = Scenario::authenticated();
    scenario1.set_last_screenshot_at_ms(Some(0));
    scenario1.persist().expect("persist state");
    let state_dir = scenario1.state_dir_path().to_path_buf();

    // Second service loads the persisted state and should not take a new screenshot
    // at t=30_000 because last_screenshot=Some(0) and interval=60 s.
    let mut scenario2 = Scenario::authenticated_with_state_dir(state_dir);
    let uploads_before = {
        let s = scenario2.api.state();
        s.batch_uploads.len() + s.hash_uploads.len()
    };
    scenario2.at_t(30_000).loop_iteration();
    let uploads_after = {
        let s = scenario2.api.state();
        s.batch_uploads.len() + s.hash_uploads.len()
    };

    // scenario1 must stay alive until scenario2 is done so the state_dir isn't deleted.
    drop(scenario1);

    assert_eq!(
        uploads_after, uploads_before,
        "no new uploads expected when screenshot interval has not elapsed after restart"
    );
}
