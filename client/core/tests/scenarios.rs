//! Behavioral scenario tests using the `Scenario` DSL.
//!
//! Each scenario uses `virtue_core::testing::Scenario` to drive a fully
//! assembled `Daemon` through a sequence of ticks without real HTTP, real
//! screenshots, real random draws, or wall-clock time.
//!
//! Run with:
//!   cargo test -p virtue-core --features testing --test scenarios

use virtue_core::module::upload::Upload;
use virtue_core::testing::Scenario;
use virtue_core::{CoreError, UploadKind};

// ── Basic unauthenticated loop ────────────────────────────────────────────────

#[test]
fn fresh_unauthenticated_service_loops_cleanly_with_no_uploads() {
    let mut scenario = Scenario::new();

    scenario
        .at_t(0)
        .tick()
        .at_t(60_000)
        .tick()
        .at_t(120_000)
        .tick();

    let status = scenario.status();
    assert!(status.is_running);
    assert!(!status.is_authenticated);
    scenario.assert_batch_upload_count(0).assert_notify_count(0);
}

// ── Screenshots ────────────────────────────────────────────────────────────────

#[test]
fn screenshot_taken_immediately_on_first_loop() {
    let mut scenario = Scenario::authenticated();
    scenario.at_t(0).tick();
    let has_upload = {
        let s = scenario.api.state();
        !s.batch_uploads.is_empty() || !s.hash_uploads.is_empty()
    };
    assert!(
        has_upload,
        "expected at least one upload after first authenticated tick"
    );
}

#[test]
fn screenshot_not_retaken_before_interval_elapses() {
    let mut scenario = Scenario::authenticated();
    scenario.with_state_mut(|s| s.screenshot.next_screenshot_at_ms = Some(60_000));
    let uploads_before = {
        let s = scenario.api.state();
        s.batch_uploads.len() + s.hash_uploads.len()
    };

    scenario.at_t(30_000).tick();

    let uploads_after = {
        let s = scenario.api.state();
        s.batch_uploads.len() + s.hash_uploads.len()
    };
    assert_eq!(
        uploads_after, uploads_before,
        "no new uploads expected before the drawn screenshot time"
    );
}

#[test]
fn screenshot_retaken_after_interval() {
    let mut scenario = Scenario::authenticated();
    scenario.with_state_mut(|s| {
        s.screenshot.next_screenshot_at_ms = Some(30_000);
        s.upload.last_batch_at_ms = Some(0);
    });

    scenario.at_t(60_001).tick();

    let has_upload = {
        let s = scenario.api.state();
        !s.batch_uploads.is_empty() || !s.hash_uploads.is_empty()
    };
    assert!(has_upload, "expected screenshot upload once due");
}

// ── User-initiated stop (unrelated to the late-wakeup model, preserved) ────────

#[test]
fn user_stop_emits_high_risk_upload() {
    let mut scenario = Scenario::authenticated();
    scenario.note_user_stop("test");
    scenario.at_t(0).tick();
    assert!(
        !scenario.api.state().notify_calls.is_empty(),
        "expected an immediate notification for UserStop alert"
    );
}

// ── Late-wakeup model (client/core/SPEC.md §2) ─────────────────────────────────

#[test]
fn late_wakeup_over_one_minute_triggers_alert() {
    let mut scenario = Scenario::authenticated();
    // First tick establishes a scheduled `next_wakeup_at_ms`.
    scenario.at_t(0).tick();
    let expected = scenario.state().next_wakeup_at_ms;

    // Wake up 70s later than scheduled: over the 1-minute single-wakeup threshold.
    scenario.at_t(expected + 70_000).tick();

    assert!(
        scenario
            .state()
            .lifecycle
            .late_wakeups
            .iter()
            .any(|&d| d > 60_000),
        "expected the lateness to be recorded"
    );
    let has_upload = {
        let s = scenario.api.state();
        !s.hash_uploads.is_empty() || !s.batch_uploads.is_empty()
    };
    assert!(
        has_upload,
        "late-wakeup alert should be recorded via the upload pipeline"
    );
}

#[test]
fn late_wakeup_near_login_is_excused() {
    let mut scenario = Scenario::authenticated();
    scenario.at_t(0).tick();
    let expected = scenario.state().next_wakeup_at_ms;
    let late_at = expected + 70_000;

    scenario.platform.set_last_login(Some(late_at));
    scenario.at_t(late_at).tick();

    assert!(
        scenario.state().lifecycle.late_wakeups.is_empty(),
        "a wakeup near a system login must be excused, not recorded"
    );
}

#[test]
fn lifecycle_disabled_never_records_or_alerts_regardless_of_lateness() {
    // Mirrors IosPlatformHooks::lifecycle_enabled() -> false.
    let mut scenario = Scenario::authenticated();
    scenario.platform.set_lifecycle_enabled(false);
    scenario.at_t(0).tick();
    let expected = scenario.state().next_wakeup_at_ms;

    // Wildly late — would easily cross the 1-minute single-wakeup threshold
    // if the check ran at all.
    scenario.at_t(expected + 10 * 60_000).tick();

    assert!(
        scenario.state().lifecycle.late_wakeups.is_empty(),
        "lifecycle_enabled(false) must skip the late-wakeup check entirely"
    );
}

// ── CaptureAvailability ─────────────────────────────────────────────────────────

fn force_screenshot_due(scenario: &mut Scenario) {
    scenario.with_state_mut(|s| s.screenshot.next_screenshot_at_ms = None);
}

#[test]
fn four_capture_failures_below_threshold_no_upload() {
    let mut scenario = Scenario::authenticated();
    for _ in 0..4 {
        scenario
            .platform
            .queue_screenshot(Err(CoreError::CommandFailed("boom".into())));
    }
    for i in 0..4 {
        force_screenshot_due(&mut scenario);
        scenario.at_t((i + 1) * 1_000).tick();
    }
    let state = scenario.api.state();
    assert!(
        state.hash_uploads.is_empty() && state.batch_uploads.is_empty(),
        "4 failures < threshold, no upload expected"
    );
}

#[test]
fn five_capture_failures_triggers_upload() {
    let mut scenario = Scenario::authenticated();
    for _ in 0..5 {
        scenario
            .platform
            .queue_screenshot(Err(CoreError::CommandFailed("boom".into())));
    }
    for i in 0..5 {
        force_screenshot_due(&mut scenario);
        scenario.at_t((i + 1) * 1_000).tick();
    }
    let state = scenario.api.state();
    assert!(
        !state.hash_uploads.is_empty() || !state.batch_uploads.is_empty(),
        "5 failures == threshold should trigger an upload"
    );
}

// ── Upload: batching ──────────────────────────────────────────────────────────

#[test]
fn low_risk_upload_queued_until_batch_interval() {
    let mut scenario = Scenario::authenticated();
    scenario.with_state_mut(|s| s.upload.last_batch_at_ms = Some(0));
    scenario.queue_upload(Upload {
        risk: 0.0,
        kind: UploadKind::Dev {
            title: "test-event".into(),
            details: None,
        },
    });
    scenario.at_t(30_000).tick();
    assert!(
        !scenario.api.state().hash_uploads.is_empty(),
        "hash should upload promptly for low-risk events"
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
    scenario.with_state_mut(|s| s.upload.last_batch_at_ms = Some(0));
    scenario.queue_upload(Upload {
        risk: 0.0,
        kind: UploadKind::Dev {
            title: "test-event".into(),
            details: None,
        },
    });
    scenario.at_t(30_000).tick();
    assert_eq!(scenario.api.state().batch_uploads.len(), 0);
    scenario.at_t(60_000).tick();
    assert!(
        !scenario.api.state().batch_uploads.is_empty(),
        "batch should flush after 60 s interval"
    );
}

#[test]
fn logout_clears_pending_state() {
    let mut scenario = Scenario::authenticated();
    scenario.with_state_mut(|s| s.upload.last_batch_at_ms = Some(0));
    scenario.queue_upload(Upload {
        risk: 0.0,
        kind: UploadKind::Dev {
            title: "pending-event".into(),
            details: None,
        },
    });
    scenario.at_t(30_000).tick();
    assert_eq!(
        scenario.api.state().batch_uploads.len(),
        0,
        "precondition: batch not yet flushed"
    );

    scenario.logout().expect("logout must succeed");
    assert!(
        scenario.state().upload.pending_batch_events.is_empty(),
        "pending batch events should be discarded on logout"
    );
    assert_eq!(
        scenario.api.state().batch_uploads.len(),
        0,
        "no batch upload should have been triggered by logout"
    );
}

// ── State persistence ─────────────────────────────────────────────────────────

#[test]
fn screenshot_state_survives_restart() {
    // Create first service, backdate the schedule so the next screenshot is
    // due well in the future.
    let mut scenario1 = Scenario::authenticated();
    scenario1.with_state_mut(|s| s.screenshot.next_screenshot_at_ms = Some(60_000));
    scenario1.at_t(0).tick(); // persists the seeded state via a no-op tick
    let state_dir = scenario1.state_dir_path().to_path_buf();

    // Second service loads the persisted state and should not take a new
    // screenshot at t=30_000 because the drawn time (60_000) hasn't arrived.
    let mut scenario2 = Scenario::authenticated_with_state_dir(state_dir);
    let uploads_before = {
        let s = scenario2.api.state();
        s.batch_uploads.len() + s.hash_uploads.len()
    };
    scenario2.at_t(30_000).tick();
    let uploads_after = {
        let s = scenario2.api.state();
        s.batch_uploads.len() + s.hash_uploads.len()
    };

    // scenario1 must stay alive until scenario2 is done so the state_dir isn't deleted.
    drop(scenario1);

    assert_eq!(
        uploads_after, uploads_before,
        "no new uploads expected when the screenshot schedule hasn't elapsed after restart"
    );
}
