//! The client API (CORE-007 and its methods), driven through the core's public
//! `Daemon` methods with a loop thread where the method needs one, and through
//! the `Scenario` DSL otherwise.
//!
//! Run with:
//!   cargo test -p virtue-core --features testing --test client_api
#![cfg(feature = "testing")]

mod common;

use std::time::{Duration, Instant};

use common::*;
use virtue_core::CoreError;
use virtue_core::testing::Scenario;

// ── CORE-007: how the methods behave ─────────────────────────────────────────

// CORE-007: a blocking request has been applied and persisted by the time it returns.
#[test]
fn blocking_requests_are_persisted_before_they_return() {
    let scenario = held_scenario();

    std::thread::scope(|scope| {
        let handle = scope.spawn(|| scenario.daemon.run_forever());
        let daemon = &scenario.daemon;

        daemon.note_user_stop("test");
        assert!(
            persisted(&scenario).lifecycle.monitoring_stopped,
            "note_user_stop"
        );

        daemon.note_user_start();
        assert!(
            !persisted(&scenario).lifecycle.monitoring_stopped,
            "note_user_start"
        );

        daemon.queue_upload(dev("persist me"));
        assert!(
            queued_events_in(&persisted(&scenario))
                .iter()
                .any(|e| e.title.as_deref() == Some("persist me")),
            "queue_upload"
        );

        daemon.force_capture_now();
        assert_eq!(
            persisted(&scenario).screenshot.last_capture_at_ms,
            Some(T0),
            "force_capture_now"
        );

        daemon.request_stop();
        handle.join().expect("loop must stop cleanly");
    });
}

// CORE-007: flush_batch_now blocks until the loop has applied it, then the batch goes out.
#[test]
fn flush_batch_now_returns_once_the_loop_has_applied_it() {
    let scenario = held_scenario();
    assert!(scenario.api.state().batch_uploads.is_empty());

    std::thread::scope(|scope| {
        let handle = scope.spawn(|| scenario.daemon.run_forever());

        scenario.daemon.flush_batch_now();
        let uploaded = eventually(|| !scenario.api.state().batch_uploads.is_empty());

        scenario.daemon.request_stop();
        handle.join().expect("loop must stop cleanly");
        assert!(
            uploaded,
            "the held batch should upload without waiting for its interval"
        );
    });
}

// CORE-007: status reads the last-committed state without needing the loop.
#[test]
fn status_reads_the_committed_state_without_the_loop() {
    let scenario = held_scenario();

    // No loop thread exists to answer a request, so this would time out after
    // 30 seconds if status went through one.
    let started = Instant::now();
    let status = scenario.daemon.status();
    assert!(started.elapsed() < Duration::from_secs(2));

    assert!(status.is_authenticated);
    assert_eq!(status.last_loop_at_ms, Some(T0));
    assert_eq!(
        status.pending_batch_count,
        scenario.state().upload.pending_batch_events.len()
    );
}

// CORE-007, CORE-014: request_stop is fire-and-forget and stays queued until a loop consumes it.
#[test]
fn request_stop_does_not_block_and_stops_the_next_loop_that_starts() {
    let scenario = held_scenario();

    let started = Instant::now();
    scenario.daemon.request_stop();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "request_stop must not wait for a loop"
    );

    std::thread::scope(|scope| {
        let handle = scope.spawn(|| scenario.daemon.run_forever());
        let stopped = eventually(|| handle.is_finished());
        if !stopped {
            // Don't hang the test run on a loop that ignored the request.
            scenario.daemon.request_stop();
        }
        handle.join().expect("loop must stop cleanly");
        assert!(
            stopped,
            "the queued stop should end the loop that starts next"
        );
    });
    assert_eq!(
        scenario.state().last_tick_at_ms,
        Some(T0),
        "stopping runs no further tick"
    );
}

// ── CORE-008 / CORE-009: login and logout ────────────────────────────────────

// CORE-008: login revokes the existing session, uses the default name when blank, and enables capture.
#[test]
fn login_revokes_the_existing_session_registers_a_device_and_enables_capture() {
    let scenario = Scenario::authenticated();

    std::thread::scope(|scope| {
        let handle = scope.spawn(|| scenario.daemon.run_forever());

        let device_id = scenario
            .daemon
            .login("me@example.org", "secret", Some("   "))
            .expect("login succeeds");
        assert_eq!(device_id, "mock-device");

        scenario.daemon.request_stop();
        handle.join().expect("loop must stop cleanly");
    });

    let api = scenario.api.state();
    assert_eq!(
        api.logout_calls,
        vec!["scenario-device-refresh".to_string()],
        "the previous session is revoked first"
    );
    assert_eq!(api.register_device_calls.len(), 1);
    assert_eq!(
        api.register_device_calls[0].name, "scenario-device",
        "a blank device name falls back to the platform default"
    );
    drop(api);

    let state = persisted(&scenario);
    assert_eq!(
        state
            .auth
            .device_credentials
            .as_ref()
            .map(|c| c.device_id.as_str()),
        Some("mock-device")
    );
    assert!(state.screenshot.enabled);
}

// CORE-008: a failed login leaves the client logged out.
#[test]
fn a_failed_login_leaves_the_client_logged_out() {
    let scenario = Scenario::authenticated();
    scenario
        .api
        .program_register_device(Err(CoreError::CommandFailed("wrong password".into())));

    std::thread::scope(|scope| {
        let handle = scope.spawn(|| scenario.daemon.run_forever());

        let result = scenario.daemon.login("me@example.org", "wrong", None);
        assert!(result.is_err());

        scenario.daemon.request_stop();
        handle.join().expect("loop must stop cleanly");
    });

    let state = persisted(&scenario);
    assert!(state.auth.device_credentials.is_none());
    assert!(!scenario.daemon.status().is_authenticated);
}

// CORE-008, CORE-005: after a failed login nothing is queued, as for any logged-out client.
#[test]
fn a_failed_login_leaves_nothing_queueing_events() {
    let mut scenario = Scenario::authenticated();
    scenario
        .api
        .program_register_device(Err(CoreError::CommandFailed("wrong password".into())));
    scenario.at_t(T0);
    scenario.queue_upload(dev("before the failed login"));
    assert!(scenario.login("me@example.org", "wrong", None).is_err());

    scenario.queue_upload(dev("after the failed login"));
    assert!(scenario.state().upload.device_credentials.is_none());
    assert!(queued_events(&scenario).is_empty());
}

// CORE-008, CORE-009: a failed login stops capture and clears the account, as a logout does.
#[test]
fn a_failed_login_stops_capture_and_clears_the_account() {
    let mut scenario = Scenario::authenticated();
    scenario
        .api
        .program_register_device(Err(CoreError::CommandFailed("wrong password".into())));
    scenario.at_t(T0);
    assert!(scenario.state().screenshot.enabled);
    assert!(scenario.login("me@example.org", "wrong", None).is_err());

    let state = scenario.state();
    assert!(!state.screenshot.enabled);
    assert!(state.auth.account_email.is_none());
    assert!(!scenario.daemon.status().is_authenticated);
}

// CORE-009: logout revokes the session best-effort, clears the credentials and disables capture.
#[test]
fn logout_clears_credentials_and_capture_even_when_the_revoke_fails() {
    let mut scenario = held_scenario();
    scenario
        .api
        .state()
        .logout_responses
        .push_back(Err(CoreError::CommandFailed("api unreachable".into())));
    assert!(!scenario.state().upload.pending_batch_events.is_empty());

    scenario
        .logout()
        .expect("logout succeeds despite the API error");

    let state = scenario.state();
    assert_eq!(
        scenario.api.state().logout_calls.len(),
        1,
        "revoke attempted"
    );
    assert!(state.auth.device_credentials.is_none());
    assert!(state.upload.device_credentials.is_none());
    assert!(!state.screenshot.enabled);
    assert!(state.upload.pending_batch_events.is_empty());

    // Nothing is captured or queued once logged out.
    scenario.queue_upload(dev("after logout"));
    let calls_before = scenario.platform.take_call_count();
    scenario.at_t(T0 + 600_000).tick();
    assert_eq!(scenario.platform.take_call_count(), calls_before);
    assert!(queued_events(&scenario).is_empty());
}

// CORE-009: a device the server no longer knows is logged out locally.
#[test]
fn a_device_the_server_rejects_is_logged_out() {
    for status in [401, 404] {
        let mut scenario = Scenario::authenticated();
        scenario.api.program_batch(Err(CoreError::HttpStatus {
            status,
            message: "gone".into(),
        }));
        scenario.at_t(T0).tick();

        let state = scenario.state();
        assert!(
            state.auth.device_credentials.is_none(),
            "HTTP {status} on the batch upload should log the device out"
        );
        assert!(!state.screenshot.enabled);
        assert!(!scenario.status().is_authenticated);
    }
}

// ── CORE-011 / CORE-022: user stop and start ─────────────────────────────────

// CORE-011: note_user_stop records the stop and alerts at once, without waiting for the batch interval.
#[test]
fn note_user_stop_records_the_stop_and_sends_an_immediate_alert() {
    let mut scenario = Scenario::authenticated();
    hold_batches(&mut scenario);

    scenario.at_t(T0).note_user_stop("tray").tick();

    assert!(scenario.state().lifecycle.monitoring_stopped);
    assert!(persisted(&scenario).lifecycle.monitoring_stopped);
    let api = scenario.api.state();
    assert_eq!(
        api.batch_uploads.len(),
        1,
        "the alert is not held for the interval"
    );
    assert_eq!(api.notify_calls.len(), 1);
    assert_eq!(api.notify_calls[0].payload.event_type, "user_stop");
    assert!(api.notify_calls[0].payload.risk >= 0.9);
}

// CORE-022: note_user_start resumes tamper detection and sends user_start.
#[test]
fn note_user_start_after_a_stop_sends_user_start_and_resumes_detection() {
    let mut scenario = held_scenario();
    scenario.at_t(T0 + 1_000).note_user_stop("test").tick();
    hold_batches(&mut scenario);

    scenario.note_user_start();

    assert!(!scenario.state().lifecycle.monitoring_stopped);
    assert_eq!(queued_count(&scenario, "user_start"), 1);

    // The time spent stopped is not measured against the old schedule.
    scenario.at_t(T0 + 3_600_000).tick();
    assert_eq!(missed_alerts(&scenario), 0);

    // Detection is back on: a late wakeup now counts.
    tick_with_lateness(&mut scenario, &[600_000]);
    assert_eq!(missed_alerts(&scenario), 1);
}

// CORE-022: with no stop active note_user_start does nothing.
#[test]
fn note_user_start_without_a_stop_sends_nothing_and_keeps_the_schedule() {
    let mut scenario = held_scenario();
    let before = queued_events(&scenario);
    let scheduled = wakeup(&scenario);

    scenario.note_user_start();

    assert_eq!(queued_events(&scenario), before);
    assert_eq!(wakeup(&scenario), scheduled);
    // The schedule is intact, so a late wakeup is still caught.
    tick_with_lateness(&mut scenario, &[600_000]);
    assert_eq!(missed_alerts(&scenario), 1);
}

// CORE-022: a restart after a user stop sends user_start.
#[test]
fn restarting_after_a_user_stop_sends_user_start() {
    let mut first = held_scenario();
    first.at_t(T0 + 1_000).note_user_stop("test").tick();

    let second =
        Scenario::authenticated_with_state_dir_at(first.state_dir_path().to_path_buf(), T0 + 5_000);

    assert!(!second.state().lifecycle.monitoring_stopped);
    assert_eq!(queued_count(&second, "user_start"), 1);
}

// CORE-022: queue_note_user_start does not wait for a loop and is applied before anything queued after it.
#[test]
fn queue_note_user_start_is_applied_before_requests_queued_after_it() {
    let mut scenario = held_scenario();
    scenario.at_t(T0 + 1_000).note_user_stop("test").tick();
    hold_batches(&mut scenario);
    scenario.tick(); // persists the held batch for the loop-less requests below
    assert!(scenario.state().lifecycle.monitoring_stopped);

    let started = Instant::now();
    scenario.daemon.queue_note_user_start();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the queued form must not wait for a loop"
    );
    assert!(
        scenario.state().lifecycle.monitoring_stopped,
        "not applied yet"
    );

    // A caller such as `login` queues behind it and blocks until a loop runs.
    std::thread::scope(|scope| {
        let handle = scope.spawn(|| scenario.daemon.queue_upload(dev("after the resume")));
        while !handle.is_finished() {
            scenario.daemon.tick_once();
            std::thread::sleep(Duration::from_millis(5));
        }
        handle.join().expect("queue_upload returns");
    });

    assert!(!scenario.state().lifecycle.monitoring_stopped);
    let kinds: Vec<String> = queued_events(&scenario)
        .into_iter()
        .map(|e| e.title.unwrap_or(e.kind))
        .collect();
    let resumed = kinds.iter().position(|k| k == "user_start");
    let after = kinds.iter().position(|k| k == "after the resume");
    assert!(
        resumed.is_some() && after.is_some() && resumed < after,
        "user_start must be queued ahead of the later request: {kinds:?}"
    );
}
