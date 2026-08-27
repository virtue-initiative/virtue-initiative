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
use virtue_core::{CoreError, StatusSkipReason, UploadKind};

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

#[test]
fn user_stop_excuse_survives_across_a_real_restart_not_just_the_next_tick() {
    // Regression test: `note_user_stop`'s excuse was previously consumed by
    // whatever tick ran next -- even one firing while the daemon is still
    // shutting down, well before the process actually exits -- leaving the
    // real stopped-time gap unprotected and reported as tampering on the
    // next launch. See CORE-002 and `lifecycle::note_user_start`.
    let mut scenario1 = Scenario::authenticated();
    scenario1.at_t(0).tick(); // establishes a schedule
    scenario1.note_user_stop("test");

    // A couple of ticks happen before the process actually exits (e.g.
    // while `request_stop` is being serviced) -- these must not consume the
    // excuse.
    scenario1.at_t(100).tick();
    scenario1.at_t(200).tick();
    assert!(scenario1.state().lifecycle.monitoring_stopped);

    let state_dir = scenario1.state_dir_path().to_path_buf();

    // Time passes while the process is down, then it restarts -- a fresh
    // `Daemon::new` via a fresh `Scenario`, exactly like a real relaunch.
    let mut scenario2 = Scenario::authenticated_with_state_dir(state_dir);
    assert!(!scenario2.state().lifecycle.monitoring_stopped);

    // Push the next screenshot draw well past this test's tick, and drop the
    // `UserStart` event `Daemon::new` just queued (covered precisely by
    // `lifecycle::tests::ticks_before_the_daemon_actually_stops_do_not_consume_the_excuse`),
    // so the only possible source of an upload below is the lifecycle
    // late-wakeup alert this test is checking for.
    scenario2.with_state_mut(|s| {
        s.screenshot.next_screenshot_at_ms = Some(1_000_000_000);
        s.upload.pending_hash_events.clear();
        s.upload.pending_batch_events.clear();
        s.upload.force_flush = false;
        s.upload.bypass_lock = false;
    });

    let uploads_before = {
        let s = scenario2.api.state();
        s.batch_uploads.len() + s.hash_uploads.len()
    };

    // First tick of the new session, an hour after the old schedule -- must
    // still be excused rather than reported as a missed wakeup.
    scenario2.at_t(3_600_000).tick();

    let uploads_after = {
        let s = scenario2.api.state();
        s.batch_uploads.len() + s.hash_uploads.len()
    };
    assert_eq!(
        uploads_before, uploads_after,
        "the gap caused by a legitimate user stop must not be reported as a missed wakeup on restart"
    );
    assert!(scenario2.state().lifecycle.late_wakeups.is_empty());
}

// ── Late-wakeup model (CORE-002) ─────────────────────────────────

#[test]
fn late_wakeup_over_two_minutes_triggers_alert() {
    let mut scenario = Scenario::authenticated();
    // First tick establishes a scheduled `next_wakeup_at_ms`.
    scenario.at_t(0).tick();
    let expected = scenario.state().next_wakeup_at_ms;

    // Wake up 130s later than scheduled: over the 2-minute single-wakeup threshold.
    scenario.at_t(expected + 130_000).tick();

    // CORE-002: cleared after the alert is sent, to prevent duplicates.
    assert!(
        scenario.state().lifecycle.late_wakeups.is_empty(),
        "expected the late wakeups array to be cleared after alerting"
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

    // Wildly late — would easily cross the 2-minute single-wakeup threshold
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

// ── Forced capture ───────────────────────────────────────────────────────────

#[test]
fn forced_capture_uploads_immediately_bypassing_interval_and_dedup() {
    let mut scenario = Scenario::authenticated();
    // Establish a schedule and an uploaded fingerprint so a normal tick
    // right now would neither be due nor pass the fingerprint diff gate.
    scenario.at_t(0).tick();
    scenario.with_state_mut(|s| {
        s.screenshot.next_screenshot_at_ms = Some(1_000_000_000);
    });
    let uploads_before = {
        let s = scenario.api.state();
        s.batch_uploads.len() + s.hash_uploads.len()
    };

    scenario.at_t(1_000).force_capture_now().tick();

    let uploads_after = {
        let s = scenario.api.state();
        s.batch_uploads.len() + s.hash_uploads.len()
    };
    assert!(
        uploads_after > uploads_before,
        "forced capture should upload even though the interval hasn't elapsed"
    );
    assert_eq!(
        scenario.state().screenshot.next_screenshot_at_ms,
        Some(1_000_000_000),
        "forced capture must not disturb the normal capture schedule"
    );
}

#[test]
fn forced_capture_flushes_the_batch_without_waiting_for_the_interval() {
    let mut scenario = Scenario::authenticated();
    scenario.with_state_mut(|s| s.upload.last_batch_at_ms = Some(0));

    scenario.at_t(1_000).force_capture_now().tick();

    assert!(
        !scenario.api.state().batch_uploads.is_empty(),
        "forced capture should flush the batch immediately rather than waiting for the batch interval"
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

// ── Status page data (CORE-010 / CORE-018) ────────────────────────────────────

#[test]
fn status_reports_the_account_device_and_last_screenshot_after_a_capture() {
    let mut scenario = Scenario::authenticated();
    scenario.at_t(1_000).tick();

    let status = scenario.status();
    assert!(status.is_authenticated);
    assert_eq!(
        status.account_email.as_deref(),
        Some("scenario@example.org")
    );
    assert_eq!(status.device_name.as_deref(), Some("scenario device"));
    // One wrapping key (the owner's own) means no partners yet.
    assert_eq!(status.partner_count, Some(0));
    assert_eq!(status.last_screenshot_attempt_at_ms, Some(1_000));
    assert_eq!(status.last_screenshot_at_ms, Some(1_000));
    assert_eq!(status.last_skip_reason, None);
    assert!(status.capture_interval_seconds > 0);
    assert!(!status.api_base_url.is_empty());
}

#[test]
fn status_reports_a_skip_reason_when_the_screen_is_locked() {
    let mut scenario = Scenario::authenticated();
    scenario.platform.set_locked_or_screensaver(true);
    scenario.at_t(1_000).tick();

    let status = scenario.status();
    assert_eq!(
        status.last_skip_reason,
        Some(StatusSkipReason::LockedOrScreensaver)
    );
    assert_eq!(status.last_screenshot_attempt_at_ms, Some(1_000));
    assert_eq!(status.last_screenshot_at_ms, None);

    // Unlocking and capturing clears the reason rather than leaving it stale.
    scenario.platform.set_locked_or_screensaver(false);
    scenario.with_state_mut(|s| s.screenshot.next_screenshot_at_ms = None);
    scenario.at_t(2_000).tick();
    assert_eq!(scenario.status().last_skip_reason, None);
}

#[test]
fn status_reports_recent_upload_errors_and_they_survive_a_restart() {
    let mut scenario = Scenario::authenticated();
    scenario
        .api
        .program_hash(Err(CoreError::CommandFailed("hash server down".into())));
    scenario.at_t(1_000).tick();

    let status = scenario.status();
    let error = status
        .recent_errors
        .first()
        .expect("a failed hash upload should be recorded");
    assert_eq!(error.context, "hash_upload");
    assert!(
        error.message.contains("hash server down"),
        "unexpected message: {}",
        error.message
    );
    assert!(status.pending_hash_count > 0);

    // The ring is persisted, so a daemon that has just restarted can still
    // explain why it is behind (CORE-018).
    let state_dir = scenario.state_dir_path().to_path_buf();
    let mut restarted = Scenario::authenticated_with_state_dir(state_dir);
    let after_restart = restarted.status();
    drop(scenario);
    assert_eq!(
        after_restart
            .recent_errors
            .first()
            .map(|e| e.context.clone()),
        Some("hash_upload".to_string())
    );
}
