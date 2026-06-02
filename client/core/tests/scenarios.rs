//! Behavioral scenario tests for `MonitorService`.
//!
//! These run on every PR via `cargo test -p virtue-core --features testing
//! --test scenarios`. Each scenario uses the `Scenario` DSL from
//! `virtue_core::testing::scenario` to drive the service through a sequence
//! of events without real HTTP, real screenshots, or wall-clock time.
//!
//! Add new scenarios here; do not put them in the in-crate `#[cfg(test)]`
//! modules. The integration-test boundary forces the DSL to be reachable
//! through `virtue_core`'s public API only, which is the same surface
//! platform crates would use.

use virtue_core::events::{Event, ProcessStoppedReason, UploadKind};
use virtue_core::module::lifecycle::{LifecycleObserverState, LifecycleStatus};
use virtue_core::testing::Scenario;

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
        .assert_log_upload_count(0)
        .assert_errors_log_empty();
}

#[test]
fn service_stop_transitions_service_to_not_running() {
    let mut scenario = Scenario::authenticated();

    scenario
        .assert_is_running(true)
        .assert_is_authenticated(true);

    scenario
        .at_t(180_000)
        .queue_event(Event::ProcessStopped(ProcessStoppedReason::Shutdown));
    let _ = scenario.service.run_event_loop_iter();
    let _ = scenario.service.mark_stopped();

    scenario.assert_is_running(false);
}

// ── ScreenshotObserver ────────────────────────────────────────────────────────

#[test]
fn screenshot_taken_immediately_on_first_loop() {
    let mut scenario = Scenario::authenticated();
    scenario.at_t(0).loop_iteration();
    scenario.assert_screenshot_call_count(1);
}

#[test]
fn screenshot_not_retaken_before_interval_elapses() {
    let mut scenario = Scenario::authenticated();
    scenario.at_t(0).loop_iteration();
    scenario.at_t(30_000).loop_iteration();
    scenario.assert_screenshot_call_count(1);
}

#[test]
fn screenshot_retaken_after_interval() {
    let mut scenario = Scenario::authenticated();
    scenario.at_t(0).loop_iteration();
    scenario.at_t(60_000).loop_iteration();
    scenario.assert_screenshot_call_count(2);
}

// ── LifecycleObserver ─────────────────────────────────────────────────────────

#[test]
fn process_started_emits_lifecycle_upload() {
    let mut scenario = Scenario::authenticated();
    scenario.queue_event(Event::ProcessStarted);
    // t=0 keeps boot-gap check below 10 s threshold so only the lifecycle
    // upload fires, not an UnexpectedProcessStart alert.
    scenario.at_t(0).loop_iteration();
    assert!(
        scenario.api.state().hash_uploads.len() >= 1,
        "expected at least one hash upload from the Lifecycle event"
    );
}

#[test]
fn user_stopped_process_emits_high_risk_upload() {
    let mut scenario = Scenario::authenticated();
    scenario.queue_event(Event::ProcessStopped(ProcessStoppedReason::User));
    scenario.at_t(0).loop_iteration();
    assert!(
        scenario.api.state().log_uploads.len() >= 1,
        "expected an immediate log upload for UserStoppedProcess alert"
    );
}

#[test]
fn ping_gap_while_running_emits_alert() {
    let mut scenario = Scenario::authenticated();
    // First loop establishes last_ping = 1_000.
    scenario.at_t(1_000).loop_iteration();
    // Second loop: gap = 11_000 ms > 10_000 ms threshold, no resume event →
    // PingGapWhileRunning alert fires at HIGH_RISK → immediate log upload.
    scenario.at_t(12_000).loop_iteration();
    assert!(
        scenario.api.state().log_uploads.len() >= 1,
        "expected a PingGapWhileRunning log upload"
    );
}

#[test]
fn computer_resume_suppresses_ping_gap_alert() {
    let mut scenario = Scenario::authenticated();
    scenario.at_t(1_000).loop_iteration();
    // A suspend+resume pair resets last_running_started to the resume time.
    // The Ping at t=12_000 sees ping_gap=11_000 > 10_000 but start_gap=0,
    // so the PingGapWhileRunning alert is suppressed.
    scenario.queue_event(Event::ComputerSuspended);
    scenario.queue_event(Event::ComputerResumed);
    scenario.at_t(12_000).loop_iteration();
    assert_eq!(
        scenario.api.state().log_uploads.len(),
        0,
        "expected no log upload when a computer-resume event suppresses the ping-gap alert"
    );
}

// ── CaptureAvailabilityObserver ───────────────────────────────────────────────

#[test]
fn four_capture_failures_below_threshold_no_upload() {
    let mut scenario = Scenario::authenticated();
    // Suppress screenshot so it doesn't add noise to upload counts.
    scenario.set_last_screenshot_at_ms(Some(0));
    for _ in 0..4 {
        scenario.queue_event(Event::CaptureFailed);
    }
    scenario.at_t(30_000).loop_iteration();
    let state = scenario.api.state();
    assert_eq!(
        state.hash_uploads.len(),
        0,
        "4 failures < threshold, no hash upload expected"
    );
    assert_eq!(
        state.batch_uploads.len(),
        0,
        "4 failures < threshold, no batch upload expected"
    );
    assert_eq!(
        state.log_uploads.len(),
        0,
        "4 failures < threshold, no log upload expected"
    );
}

#[test]
fn five_capture_failures_triggers_upload() {
    let mut scenario = Scenario::authenticated();
    // Suppress screenshot so upload counts reflect only the CaptureFailed
    // threshold event. Defer batch upload to keep assertions simple.
    scenario.set_last_screenshot_at_ms(Some(0));
    scenario.set_last_batch_at_ms(Some(0));
    for _ in 0..5 {
        scenario.queue_event(Event::CaptureFailed);
    }
    scenario.at_t(30_000).loop_iteration();
    assert!(
        scenario.api.state().hash_uploads.len() >= 1,
        "5 failures == threshold should trigger an Upload event that goes through hash upload"
    );
}

// ── UploadObserver ────────────────────────────────────────────────────────────

#[test]
fn low_risk_upload_queued_until_batch_interval() {
    let mut scenario = Scenario::authenticated();
    // Simulate a batch that was just flushed so the interval guard is active.
    scenario.set_last_batch_at_ms(Some(0));
    scenario.queue_event(Event::Upload {
        risk: 0.0,
        kind: UploadKind::Dev {
            title: "test-event".into(),
            details: None,
        },
    });
    scenario.at_t(30_000).loop_iteration();
    // Hash must have been uploaded; batch must still be deferred (30 s < 60 s).
    assert!(
        scenario.api.state().hash_uploads.len() >= 1,
        "hash should be uploaded immediately for low-risk events"
    );
    assert_eq!(
        scenario.api.state().batch_uploads.len(),
        0,
        "batch should not be flushed before the 60 s interval elapses"
    );
}

#[test]
fn batch_flushed_after_interval() {
    let mut scenario = Scenario::authenticated();
    scenario.set_last_batch_at_ms(Some(0));
    scenario.queue_event(Event::Upload {
        risk: 0.0,
        kind: UploadKind::Dev {
            title: "test-event".into(),
            details: None,
        },
    });
    // Batch still deferred at 30 s.
    scenario.at_t(30_000).loop_iteration();
    assert_eq!(scenario.api.state().batch_uploads.len(), 0);
    // Batch flushed once the interval is met at 60 s.
    scenario.at_t(60_000).loop_iteration();
    assert!(
        scenario.api.state().batch_uploads.len() >= 1,
        "batch should be flushed after the 60 s interval"
    );
}

#[test]
fn logout_clears_pending_state() {
    let mut scenario = Scenario::authenticated();
    // Defer the batch so there are pending batch events after the loop.
    scenario.set_last_batch_at_ms(Some(0));
    scenario.queue_event(Event::Upload {
        risk: 0.0,
        kind: UploadKind::Dev {
            title: "pending-event".into(),
            details: None,
        },
    });
    scenario.at_t(30_000).loop_iteration();
    // Hash was uploaded but the batch is still deferred (30 s < 60 s interval).
    assert_eq!(
        scenario.api.state().batch_uploads.len(),
        0,
        "precondition: no batch should have been flushed yet"
    );
    scenario.service.logout().expect("logout must succeed");
    // Logout discards pending batch events instead of flushing them.
    assert_eq!(
        scenario.api.state().batch_uploads.len(),
        0,
        "pending batch events should be discarded (not uploaded) on logout"
    );
}

// ── LifecycleObserver alert paths ─────────────────────────────────────────────

#[test]
fn ping_after_suspend_without_resume_emits_alert() {
    let mut scenario = Scenario::authenticated();
    // status=Suspended with 3 pings already counted; the next Ping from
    // loop_iteration makes it 4 > 3, triggering MissingResume (risk=0.6 →
    // hash upload).
    scenario.set_lifecycle_observer_state(LifecycleObserverState {
        status: LifecycleStatus::Suspended,
        pings_while_suspended: 3,
        ..Default::default()
    });
    scenario.at_t(10_000).loop_iteration();
    assert!(
        scenario.api.state().hash_uploads.len() >= 1,
        "expected a MissingResume hash upload after 4 pings while suspended"
    );
}

#[test]
fn unexpected_process_start_after_long_ping_gap_emits_alert() {
    let mut scenario = Scenario::authenticated();
    // ping_gap = 99_000 > 10_000; TestPlatformHooks returns boot=0 so
    // now_ms - boot = 100_000 > 10_000 → UnexpectedProcessStart fires.
    scenario.set_lifecycle_observer_state(LifecycleObserverState {
        last_ping: 1_000,
        ..Default::default()
    });
    scenario.queue_event(Event::ProcessStarted);
    scenario.at_t(100_000).loop_iteration();
    assert!(
        scenario.api.state().log_uploads.len() >= 1,
        "expected an UnexpectedProcessStart log upload"
    );
}

#[test]
fn process_killed_before_shutdown_emits_alert() {
    let mut scenario = Scenario::authenticated();
    // stopped_other=1_000 > started=0 and last_shutdown=12_000 - stopped=1_000 = 11_000 > 10_000
    // → ProcessKilledBeforeShutdown fires on the next event (auto-Ping from iter).
    scenario.set_lifecycle_observer_state(LifecycleObserverState {
        last_process_stopped_other: 1_000,
        last_process_stopped_shutdown: 12_000,
        ..Default::default()
    });
    scenario.at_t(20_000).loop_iteration();
    assert!(
        scenario.api.state().hash_uploads.len() >= 1,
        "expected a ProcessKilledBeforeShutdown hash upload"
    );
}

// ── State persistence ─────────────────────────────────────────────────────────

#[test]
fn screenshot_state_survives_restart() {
    // First service: take one screenshot at t=0 and let the state persist.
    let mut scenario1 = Scenario::authenticated();
    scenario1.at_t(0).loop_iteration();
    scenario1.assert_screenshot_call_count(1);

    let state_dir = scenario1.state_dir_path().to_path_buf();

    // Second service: loads persisted state (last_screenshot_at_ms = Some(0)).
    // At t=30_000 the interval (60 s) has not elapsed → no screenshot.
    let mut scenario2 = Scenario::authenticated_with_state_dir(state_dir);
    scenario2.at_t(30_000).loop_iteration();
    scenario2.assert_screenshot_call_count(0);
}
