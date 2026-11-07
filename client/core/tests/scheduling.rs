//! Wakeup scheduling (CORE-020) and forced capture (CORE-021), driven through
//! the `Scenario` DSL.
//!
//! Run with:
//!   cargo test -p virtue-core --features testing --test scheduling
#![cfg(feature = "testing")]

mod common;

use std::time::Duration;

use common::*;
use virtue_core::CoreError;
use virtue_core::testing::Scenario;

const BATCH_INTERVAL_MS: i64 = 60_000;

// ── The wakeup is the earliest pending timer ─────────────────────────────────

// CORE-020: the wakeup is the earliest of the next screenshot, heartbeat and batch interval.
#[test]
fn the_wakeup_is_the_earliest_of_the_screenshot_heartbeat_and_batch_timers() {
    // Offsets from the tick time of each timer: the next screenshot draw, the
    // next heartbeat, and the end of the pending batch's interval.
    struct Timers {
        name: &'static str,
        screenshot: i64,
        heartbeat: i64,
        batch: i64,
    }
    let cases = [
        Timers {
            name: "the screenshot draw is earliest",
            screenshot: 20_000,
            heartbeat: 40_000,
            batch: 30_000,
        },
        Timers {
            name: "the batch interval ending is earliest",
            screenshot: 50_000,
            heartbeat: 40_000,
            batch: 30_000,
        },
        Timers {
            name: "the heartbeat is earliest",
            screenshot: 50_000,
            heartbeat: 10_000,
            batch: 30_000,
        },
    ];

    for case in &cases {
        let mut scenario = Scenario::authenticated();
        scenario.with_state_mut(|s| {
            s.screenshot.next_screenshot_at_ms = Some(T0 + case.screenshot);
            s.heartbeat.last_heartbeat_ms = T0 + case.heartbeat - DAY_MS;
            s.upload.last_batch_at_ms = Some(T0 + case.batch - BATCH_INTERVAL_MS);
        });
        // One event that has to wait for the batch interval to end.
        scenario.queue_upload(dev("waiting for the batch"));
        scenario.at_t(T0).tick();

        assert_eq!(
            scenario.state().upload.pending_batch_events.len(),
            1,
            "{}: the event should be hashed and waiting for its batch",
            case.name
        );
        let earliest = case.screenshot.min(case.heartbeat).min(case.batch);
        assert_eq!(wakeup(&scenario), T0 + earliest, "{}", case.name);
    }
}

// CORE-020: a hash retry backoff expiring is a wakeup time.
#[test]
fn a_hash_retry_backoff_expiring_sets_the_wakeup() {
    let mut scenario = Scenario::authenticated();
    scenario.with_state_mut(|s| s.screenshot.next_screenshot_at_ms = Some(T0 + 500_000));
    scenario
        .api
        .program_hash(Err(CoreError::CommandFailed("hash server down".into())));
    scenario.queue_upload(dev("hash me"));
    scenario.at_t(T0).tick();

    let state = scenario.state();
    let retry_at = state.upload.hash_backoff.next_attempt_at_ms;
    assert!(retry_at > T0, "the failed hash must not be retried at once");
    assert!(retry_at < T0 + 500_000);
    assert_eq!(state.next_wakeup_at_ms, retry_at);
}

// CORE-020: a batch retry backoff expiring is a wakeup time.
#[test]
fn a_batch_retry_backoff_expiring_sets_the_wakeup() {
    let mut scenario = Scenario::authenticated();
    scenario.with_state_mut(|s| s.screenshot.next_screenshot_at_ms = Some(T0 + 500_000));
    scenario
        .api
        .program_batch(Err(CoreError::CommandFailed("api down".into())));
    scenario.queue_upload(dev("batch me"));
    scenario.at_t(T0).tick();

    let state = scenario.state();
    let retry_at = state.upload.batch_backoff.next_attempt_at_ms;
    assert!(
        retry_at > T0,
        "the failed batch must not be retried at once"
    );
    assert!(retry_at < T0 + 500_000);
    assert_eq!(state.upload.pending_batch_events.len(), 1);
    assert_eq!(state.next_wakeup_at_ms, retry_at);
}

// CORE-020: hash work that is ready to run again makes the wakeup immediate.
#[test]
fn hash_work_left_over_after_a_tick_is_due_immediately() {
    let mut scenario = Scenario::authenticated();
    scenario.with_state_mut(|s| {
        s.screenshot.next_screenshot_at_ms = Some(T0 + 500_000);
        s.upload.last_batch_at_ms = Some(FAR_FUTURE);
    });
    // A tick hashes at most 8 events, so the rest are ready to go again.
    for i in 0..20 {
        scenario.queue_upload(dev(&format!("event {i}")));
    }
    scenario.at_t(T0).tick();

    let state = scenario.state();
    assert!(!state.upload.pending_hash_events.is_empty());
    assert_eq!(state.next_wakeup_at_ms, T0);
}

// CORE-020: a forced batch that failed to upload is next due when its backoff expires.
#[test]
fn a_forced_flush_that_failed_is_retried_when_its_backoff_expires_not_at_the_interval() {
    let mut scenario = Scenario::authenticated();
    scenario.with_state_mut(|s| {
        s.screenshot.next_screenshot_at_ms = Some(T0 + 500_000);
        s.upload.last_batch_at_ms = Some(T0);
    });
    scenario
        .api
        .program_batch(Err(CoreError::CommandFailed("api down".into())));
    scenario.queue_upload(dev("flush me"));
    scenario.at_t(T0 + 1_000).flush_batch_now().tick();

    // The interval has not elapsed (it ends at T0 + 60 s), but the forced
    // batch can be retried as soon as its backoff ends, which is sooner.
    let state = scenario.state();
    assert_eq!(state.upload.pending_batch_events.len(), 1);
    let retry_at = state.upload.batch_backoff.next_attempt_at_ms;
    assert!(retry_at < T0 + BATCH_INTERVAL_MS);
    assert_eq!(state.next_wakeup_at_ms, retry_at);
}

// ── Held work does not pull the wakeup earlier ───────────────────────────────

// CORE-020: work held by a condition no timer clears does not move the wakeup earlier.
#[test]
fn held_work_waits_for_the_next_screenshot_draw_instead_of_pulling_the_wakeup_earlier() {
    let draw = T0 + 500_000;
    let mut failures = Vec::new();

    let cases: Vec<(&str, Scenario)> = vec![
        ("a hash upload held while the screen is locked", {
            let mut s = Scenario::authenticated();
            s.with_state_mut(|st| st.screenshot.next_screenshot_at_ms = Some(draw));
            s.platform.set_locked_or_screensaver(true);
            s.queue_upload(dev("held by the lock"));
            s
        }),
        ("a due batch held while the screen is locked", {
            let mut s = Scenario::authenticated();
            s.with_state_mut(|st| {
                st.screenshot.next_screenshot_at_ms = Some(draw);
                st.upload.last_batch_at_ms = Some(T0 - 1_000);
            });
            s.queue_upload(dev("batch held by the lock"));
            // Hashed while unlocked; the batch interval ends at T0 + 59 s.
            s.at_t(T0).tick();
            s.platform.set_locked_or_screensaver(true);
            s.at_t(T0 + 70_000);
            s
        }),
        ("a pending batch with no device settings", {
            let mut s = Scenario::authenticated();
            s.with_state_mut(|st| {
                st.screenshot.next_screenshot_at_ms = Some(draw);
                st.upload.settings = None;
            });
            s.queue_upload(dev("batch with nowhere to go"));
            s
        }),
        ("pending hashes with no device credentials", {
            let mut s = Scenario::authenticated();
            s.with_state_mut(|st| st.screenshot.next_screenshot_at_ms = Some(draw));
            s.queue_upload(dev("hash with no credentials"));
            s.with_state_mut(|st| st.upload.device_credentials = None);
            s
        }),
    ];

    for (name, mut scenario) in cases {
        let now = scenario.clock.now_ms().max(T0);
        scenario.at_t(now).tick();

        let state = scenario.state();
        let held = state.upload.pending_hash_events.len() + state.upload.pending_batch_events.len();
        if held == 0 {
            failures.push(format!("{name}: precondition failed, nothing was held"));
        }
        if state.next_wakeup_at_ms != draw {
            failures.push(format!(
                "{name}: wakeup {} should be the screenshot draw {draw} (now {now})",
                state.next_wakeup_at_ms
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ── The loop does not tick before the wakeup ─────────────────────────────────

// CORE-020: the loop does not run a tick before the wakeup time, even though it wakes up.
#[test]
fn the_loop_wakes_without_ticking_until_the_wakeup_time_is_reached() {
    let scenario = held_scenario();
    let wake = wakeup(&scenario);
    let ticked_at = scenario.state().last_tick_at_ms;
    assert_eq!(ticked_at, Some(T0));
    // 150 ms of real waiting separates the loop from the wakeup time.
    scenario.clock.set(wake - 150);

    std::thread::scope(|scope| {
        let handle = scope.spawn(|| scenario.daemon.run_forever());

        // The loop wakes several times in this span and finds nothing due.
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(
            scenario.state().last_tick_at_ms,
            ticked_at,
            "no tick may run before the wakeup time"
        );

        scenario.clock.set(wake);
        let ticked = eventually(|| scenario.state().last_tick_at_ms == Some(wake));

        scenario.daemon.request_stop();
        handle.join().expect("loop must stop cleanly");
        assert!(ticked, "a tick should run once the wakeup time is reached");
    });
}

// CORE-020: a client request that was applied runs a tick before the wakeup time.
#[test]
fn an_applied_client_request_ticks_before_the_wakeup_time() {
    let scenario = held_scenario();
    let wake = wakeup(&scenario);
    // Far enough from the wakeup that only the request can cause a tick.
    scenario.clock.set(wake - 30_000);

    std::thread::scope(|scope| {
        let handle = scope.spawn(|| scenario.daemon.run_forever());

        scenario.daemon.flush_batch_now();
        let ticked = eventually(|| scenario.state().last_tick_at_ms == Some(wake - 30_000));

        scenario.daemon.request_stop();
        handle.join().expect("loop must stop cleanly");
        assert!(ticked, "applying a request should run a tick");
    });
}

// ── Forced capture (CORE-021) ────────────────────────────────────────────────

/// An authenticated scenario with a batch waiting out its interval and the next
/// scheduled capture far away, so only a forced capture can take a screenshot.
fn scenario_with_a_batch_waiting() -> Scenario {
    let mut scenario = Scenario::authenticated();
    scenario.with_state_mut(|s| {
        s.screenshot.next_screenshot_at_ms = Some(FAR_FUTURE);
        s.upload.last_batch_at_ms = Some(T0);
    });
    scenario.queue_upload(dev("waiting for the batch"));
    scenario.at_t(T0 + 1_000).tick();
    assert_eq!(scenario.state().upload.pending_batch_events.len(), 1);
    assert!(scenario.api.state().batch_uploads.is_empty());
    scenario
}

// CORE-021: a forced capture still honors the locked gate, yet its batch is still flushed.
#[test]
fn a_forced_capture_while_locked_takes_no_screenshot_but_still_flushes_the_batch() {
    let mut scenario = scenario_with_a_batch_waiting();
    scenario.platform.set_locked_or_screensaver(true);

    scenario.at_t(T0 + 2_000).force_capture_now().tick();

    assert_eq!(scenario.platform.take_call_count(), 0, "no screenshot");
    assert_eq!(queued_count(&scenario, "screenshot"), 0);
    assert_eq!(
        scenario.api.state().batch_uploads.len(),
        1,
        "the batch holding the forced capture is flushed even though the screen is locked"
    );
}

// CORE-021: tick_once with a forced capture honors the locked gate the same way.
#[test]
fn a_forced_tick_once_while_locked_takes_no_screenshot_but_still_flushes_the_batch() {
    let mut scenario = scenario_with_a_batch_waiting();
    scenario.platform.set_locked_or_screensaver(true);

    scenario.at_t(T0 + 2_000);
    scenario.daemon.tick_once_forced();

    assert_eq!(scenario.platform.take_call_count(), 0);
    assert_eq!(scenario.api.state().batch_uploads.len(), 1);
}

// CORE-021: nothing is captured while logged out, forced or not.
#[test]
fn a_forced_capture_while_logged_out_takes_no_screenshot() {
    let mut scenario = Scenario::new();
    scenario.at_t(T0).force_capture_now().tick();

    assert_eq!(scenario.platform.take_call_count(), 0);
    scenario.assert_batch_upload_count(0);
}

// CORE-021: a forced capture takes a screenshot even if one is not yet due and flushes its batch.
#[test]
fn a_forced_capture_takes_a_screenshot_and_flushes_it_with_the_batch_it_joins() {
    let mut scenario = scenario_with_a_batch_waiting();

    scenario.at_t(T0 + 2_000).force_capture_now().tick();

    assert_eq!(scenario.platform.take_call_count(), 1);
    let api = scenario.api.state();
    assert_eq!(api.batch_uploads.len(), 1);
    assert_eq!(
        api.batch_uploads[0].batch.screenshot_count, 1,
        "the batch that went out holds the forced screenshot"
    );
    assert_eq!(api.batch_uploads[0].batch.total_count, 2);
}

// CORE-013: flush_batch_now uploads the current batch without waiting for the interval.
#[test]
fn flush_batch_now_uploads_the_batch_before_the_interval_ends() {
    let mut scenario = scenario_with_a_batch_waiting();

    scenario.at_t(T0 + 2_000).tick();
    assert!(
        scenario.api.state().batch_uploads.is_empty(),
        "interval not over"
    );

    scenario.at_t(T0 + 3_000).flush_batch_now().tick();
    assert_eq!(scenario.api.state().batch_uploads.len(), 1);
}
