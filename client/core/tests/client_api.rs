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
use virtue_core::testing::Scenario;
use virtue_core::{CoreError, DaemonState};

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
        "the previous session is revoked"
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

// CORE-008: a successful login tries to deliver the pending batch with the old session.
#[test]
fn a_successful_login_uploads_the_pending_batch_with_the_old_session() {
    let mut scenario = held_scenario();
    let uploaded_before = scenario.api.state().batch_uploads.len();
    assert!(!scenario.state().upload.pending_batch_events.is_empty());
    scenario
        .login("me@example.org", "secret", None)
        .expect("login succeeds");

    let api = scenario.api.state();
    assert_eq!(api.batch_uploads.len(), uploaded_before + 1);
    assert_eq!(
        api.batch_uploads[uploaded_before].device_refresh_token,
        "scenario-device-refresh"
    );
    assert_eq!(api.logout_calls.len(), 1);
    drop(api);
    assert!(scenario.state().upload.pending_batch_events.is_empty());
}

// CORE-008: events still waiting for the hash server are discarded, not flushed.
#[test]
fn a_login_discards_events_not_yet_hashed() {
    let mut scenario = Scenario::authenticated();
    scenario.at_t(T0);
    scenario.queue_upload(dev("before the login"));
    scenario
        .login("me@example.org", "secret", None)
        .expect("login succeeds");

    let api = scenario.api.state();
    assert!(api.hash_uploads.is_empty());
    assert!(api.batch_uploads.is_empty());
    drop(api);
    assert!(queued_events(&scenario).is_empty());
}

// CORE-008: a failed upload of the pending batch does not fail the login.
#[test]
fn a_login_succeeds_when_the_pending_batch_cannot_be_uploaded() {
    let mut scenario = held_scenario();
    scenario
        .api
        .program_batch(Err(CoreError::CommandFailed("api unreachable".into())));
    scenario
        .login("me@example.org", "secret", None)
        .expect("login succeeds");

    assert_eq!(scenario.api.state().logout_calls.len(), 1);
    assert!(scenario.daemon.status().is_authenticated);
    assert!(queued_events(&scenario).is_empty());
}

// CORE-008: a failed flush leaves no backoff behind, so the new session uploads straight away.
#[test]
fn the_new_session_uploads_at_once_after_a_failed_flush() {
    let mut scenario = held_scenario();
    scenario
        .api
        .program_batch(Err(CoreError::CommandFailed("api unreachable".into())));
    scenario.with_state_mut(|s| {
        s.upload.hash_backoff.current_backoff_ms = 20 * 60 * 1_000;
        s.upload.batch_backoff.current_backoff_ms = 20 * 60 * 1_000;
    });
    scenario
        .login("me@example.org", "secret", None)
        .expect("login succeeds");
    let hashed_before = scenario.api.state().hash_uploads.len();
    let uploaded_before = scenario.api.state().batch_uploads.len();

    scenario.at_t(T0 + 60_000).tick();

    let api = scenario.api.state();
    assert!(api.hash_uploads.len() > hashed_before);
    assert!(api.batch_uploads.len() > uploaded_before);
}

/// Dirties every part of the state a new session must not inherit.
fn dirty_state(scenario: &mut Scenario) {
    scenario.with_state_mut(|s| {
        s.lifecycle.late_wakeups.push_back(600_000);
        s.lifecycle.last_seen_login_ms = Some(T0);
        s.lifecycle.last_seen_logout_ms = Some(T0);
        s.lifecycle.monitoring_stopped = true;
        s.lifecycle.restart_times.push_back(T0);
        s.lifecycle.last_restart_alert_at_ms = Some(T0);
        s.screenshot.last_capture_at_ms = Some(T0);
        s.upload.next_hash_seq = 7;
        s.upload.last_batch_at_ms = Some(T0);
        s.upload.force_flush = true;
        s.upload.bypass_lock = true;
        s.upload.hash_backoff.current_backoff_ms = 60_000;
        s.upload.batch_backoff.current_backoff_ms = 60_000;
        s.capture_availability.recent_failures_ms.push(T0);
        s.heartbeat.last_heartbeat_ms = T0;
        s.next_wakeup_at_ms = T0;
        s.last_tick_at_ms = Some(T0);
    });
}

/// Fails unless everything `dirty_state` set was reset.
fn assert_old_state_is_gone(state: &DaemonState) {
    assert!(state.lifecycle.late_wakeups.is_empty());
    assert_eq!(state.lifecycle.last_seen_login_ms, None);
    assert_eq!(state.lifecycle.last_seen_logout_ms, None);
    assert!(!state.lifecycle.monitoring_stopped);
    assert!(state.lifecycle.restart_times.is_empty());
    assert_eq!(state.lifecycle.last_restart_alert_at_ms, None);
    assert_eq!(state.screenshot.last_capture_at_ms, None);
    assert_eq!(state.upload.next_hash_seq, 0);
    assert_eq!(state.upload.last_batch_at_ms, None);
    assert!(!state.upload.force_flush);
    assert!(!state.upload.bypass_lock);
    assert_eq!(state.upload.hash_backoff.current_backoff_ms, 0);
    assert_eq!(state.upload.batch_backoff.current_backoff_ms, 0);
    assert!(state.capture_availability.recent_failures_ms.is_empty());
    assert_eq!(state.heartbeat.last_heartbeat_ms, 0);
    assert_eq!(state.next_wakeup_at_ms, 0);
    assert_eq!(state.last_tick_at_ms, None);
    assert!(state.errors.recent.is_empty());
}

// CORE-008: a new session starts from a fresh state, with the old session's errors gone too.
#[test]
fn a_login_starts_from_a_fresh_state() {
    let mut scenario = Scenario::authenticated();
    scenario
        .api
        .program_hash(Err(CoreError::CommandFailed("hash server down".into())));
    scenario.at_t(T0).tick();
    assert!(!scenario.daemon.status().recent_errors.is_empty());
    dirty_state(&mut scenario);

    scenario
        .login("new@example.org", "secret", None)
        .expect("login succeeds");

    let state = scenario.state();
    assert_old_state_is_gone(&state);
    assert_eq!(state.auth.account_email.as_deref(), Some("new@example.org"));
    assert!(state.screenshot.enabled);
}

// CORE-008: a failed login keeps the existing session.
#[test]
fn a_failed_login_keeps_the_existing_session() {
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

    assert!(scenario.api.state().logout_calls.is_empty());
    let state = persisted(&scenario);
    assert!(state.auth.device_credentials.is_some());
    assert!(scenario.daemon.status().is_authenticated);
}

// CORE-008: a failed login keeps queued events and keeps queueing new ones.
#[test]
fn a_failed_login_keeps_queueing_events() {
    let mut scenario = Scenario::authenticated();
    scenario
        .api
        .program_register_device(Err(CoreError::CommandFailed("wrong password".into())));
    scenario.at_t(T0);
    scenario.queue_upload(dev("before the failed login"));
    assert!(scenario.login("me@example.org", "wrong", None).is_err());

    scenario.queue_upload(dev("after the failed login"));
    assert!(scenario.state().upload.device_credentials.is_some());
    assert_eq!(queued_events(&scenario).len(), 2);
}

// CORE-008: a failed login keeps capture running and the account named.
#[test]
fn a_failed_login_keeps_capture_and_the_account() {
    let mut scenario = Scenario::authenticated();
    scenario
        .api
        .program_register_device(Err(CoreError::CommandFailed("wrong password".into())));
    scenario.at_t(T0);
    let account_email = scenario.state().auth.account_email.clone();
    assert!(scenario.state().screenshot.enabled);
    assert!(scenario.login("me@example.org", "wrong", None).is_err());

    let state = scenario.state();
    assert!(state.screenshot.enabled);
    assert_eq!(state.auth.account_email, account_email);
    assert!(scenario.daemon.status().is_authenticated);
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

// CORE-009: logout resets the whole state, including the recent errors.
#[test]
fn logout_resets_the_whole_state() {
    let mut scenario = Scenario::authenticated();
    scenario
        .api
        .program_hash(Err(CoreError::CommandFailed("hash server down".into())));
    scenario.at_t(T0).tick();
    assert!(!scenario.daemon.status().recent_errors.is_empty());
    dirty_state(&mut scenario);

    scenario.logout().expect("logout succeeds");

    let state = scenario.state();
    assert_old_state_is_gone(&state);
    assert_eq!(state.auth.account_email, None);
}

// CORE-009: a device the server no longer knows is logged out locally, and the reason stays visible.
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
        let service_status = scenario.status();
        assert!(!service_status.is_authenticated);
        assert!(
            service_status
                .recent_errors
                .iter()
                .any(|e| e.message.contains("logging out"))
        );
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
