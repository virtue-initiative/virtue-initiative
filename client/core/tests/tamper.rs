//! Tamper detection scenarios (CORE-002) and repeated restart detection
//! (CORE-019), driven through the `Scenario` DSL: the whole daemon with a fake
//! clock and fake platform hooks.
//!
//! Run with:
//!   cargo test -p virtue-core --features testing --test tamper
#![cfg(feature = "testing")]

mod common;

use common::*;
use virtue_core::testing::Scenario;

// ── The excuse rule: login, logout and suspend evidence ───────────────────────

const LATE: i64 = 600_000; // 10 minutes late, far over the 2-minute alert threshold
const NEAR: i64 = 60_000;
const EDGE: i64 = 120_000;
const JUST_OVER: i64 = 120_001;
const STALE: i64 = 3 * DAY_MS;

/// One gap and the evidence around it. `login` is how long before the late
/// tick the platform says the last login was, `logout` how long before the
/// expected wakeup the last logout was, and `suspended` how much of the span
/// since the previous tick the suspend-safe clock missed. `None` means the
/// platform hook returned nothing.
struct Gap {
    name: &'static str,
    login: Option<i64>,
    logout: Option<i64>,
    suspended: Option<i64>,
    excused: bool,
}

const fn gap(
    name: &'static str,
    login: Option<i64>,
    logout: Option<i64>,
    suspended: Option<i64>,
    excused: bool,
) -> Gap {
    Gap {
        name,
        login,
        logout,
        suspended,
        excused,
    }
}

// CORE-002: the gap is excused only if some evidence supports it and none contradicts it.
#[test]
fn login_logout_and_suspend_evidence_decide_whether_a_gap_is_excused() {
    let gaps = [
        // No evidence.
        gap(
            "a gap with no evidence at all is recorded",
            None,
            None,
            None,
            false,
        ),
        // Login evidence alone: the current time within 2 minutes of the last login.
        gap(
            "a login within 2 minutes of now excuses the gap",
            Some(NEAR),
            None,
            None,
            true,
        ),
        gap(
            "a login exactly 2 minutes before now excuses the gap",
            Some(EDGE),
            None,
            None,
            true,
        ),
        gap(
            "a login 2 minutes and 1 ms before now does not excuse the gap",
            Some(JUST_OVER),
            None,
            None,
            false,
        ),
        gap(
            "a login days ago does not excuse the gap",
            Some(STALE),
            None,
            None,
            false,
        ),
        // Logout evidence alone: the expected wakeup within 2 minutes of the last logout.
        gap(
            "a logout within 2 minutes of the expected wakeup excuses the gap",
            None,
            Some(NEAR),
            None,
            true,
        ),
        gap(
            "a logout exactly 2 minutes before the expected wakeup excuses the gap",
            None,
            Some(EDGE),
            None,
            true,
        ),
        gap(
            "a logout 2 minutes and 1 ms before the expected wakeup does not excuse the gap",
            None,
            Some(JUST_OVER),
            None,
            false,
        ),
        gap(
            "a logout days ago does not excuse the gap",
            None,
            Some(STALE),
            None,
            false,
        ),
        // Login and logout together.
        gap(
            "supporting login and logout excuse the gap",
            Some(NEAR),
            Some(NEAR),
            None,
            true,
        ),
        gap(
            "a supporting login is blocked by a contradicting logout (killed well before a reboot)",
            Some(NEAR),
            Some(STALE),
            None,
            false,
        ),
        gap(
            "a supporting login is blocked by a logout 1 ms outside its window",
            Some(NEAR),
            Some(JUST_OVER),
            None,
            false,
        ),
        gap(
            "a supporting logout is blocked by a contradicting login (not restarted until long after)",
            Some(STALE),
            Some(NEAR),
            None,
            false,
        ),
        gap(
            "a supporting logout is blocked by a login 1 ms outside its window",
            Some(JUST_OVER),
            Some(NEAR),
            None,
            false,
        ),
        gap(
            "a stale login and a stale logout do not excuse the gap",
            Some(STALE),
            Some(STALE),
            None,
            false,
        ),
        // Suspend evidence: the suspended time must account for the whole gap.
        gap(
            "a suspend covering the whole gap excuses it",
            None,
            None,
            Some(LATE),
            true,
        ),
        gap(
            "a suspend short of the gap by only wake-up jitter excuses it",
            None,
            None,
            Some(LATE - 30_000),
            true,
        ),
        gap(
            "a suspend covering half the gap does not excuse it",
            None,
            None,
            Some(LATE / 2),
            false,
        ),
        gap(
            "a brief suspend inside a long gap does not excuse it",
            None,
            None,
            Some(30_000),
            false,
        ),
        // Suspend evidence never contradicts, only supports.
        gap(
            "a suspend excuses the gap beside a stale login",
            Some(STALE),
            None,
            Some(LATE),
            true,
        ),
        gap(
            "a suspend excuses the gap beside a stale logout",
            None,
            Some(STALE),
            Some(LATE),
            true,
        ),
        gap(
            "a suspend excuses the gap beside a stale login and logout",
            Some(STALE),
            Some(STALE),
            Some(LATE),
            true,
        ),
        gap(
            "a suspend excuses the gap beside a supporting login",
            Some(NEAR),
            None,
            Some(LATE),
            true,
        ),
        gap(
            "a brief suspend cannot block a supporting login",
            Some(NEAR),
            None,
            Some(30_000),
            true,
        ),
        gap(
            "a brief suspend cannot block a supporting logout",
            None,
            Some(NEAR),
            Some(30_000),
            true,
        ),
        gap(
            "a brief suspend beside stale records does not excuse the gap",
            Some(STALE),
            Some(STALE),
            Some(30_000),
            false,
        ),
        // A contradiction between login and logout still blocks when a suspend supports.
        gap(
            "a suspend does not rescue a supporting login blocked by a contradicting logout",
            Some(NEAR),
            Some(STALE),
            Some(LATE),
            false,
        ),
        gap(
            "a suspend does not rescue a supporting logout blocked by a contradicting login",
            Some(STALE),
            Some(NEAR),
            Some(LATE),
            false,
        ),
    ];

    let mut failures = Vec::new();
    for case in &gaps {
        let mut scenario = held_scenario();
        let expected = wakeup(&scenario);
        let now = expected + LATE;
        if let Some(before_now) = case.login {
            scenario.platform.set_last_login(Some(now - before_now));
        }
        if let Some(before_expected) = case.logout {
            scenario
                .platform
                .set_last_logout(Some(expected - before_expected));
        }
        if let Some(suspended) = case.suspended {
            // Real time passed, the suspend-safe clock missed `suspended` of it.
            scenario
                .platform
                .set_monotonic_clock_override(Some(now - suspended));
        }
        scenario.at_t(now).tick();

        let alerts = missed_alerts(&scenario);
        let recorded = scenario.state().lifecycle.late_wakeups.len();
        let ok = if case.excused {
            alerts == 0 && recorded == 0
        } else {
            // Recorded and over the threshold: alerted once, then cleared.
            alerts == 1 && recorded == 0
        };
        if !ok {
            failures.push(format!(
                "{}: expected {}, saw {alerts} alerts and {recorded} recorded entries",
                case.name,
                if case.excused { "excused" } else { "recorded" },
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ── Recording lateness and the alert thresholds ──────────────────────────────

// CORE-002: the difference is added to the late wakeups array.
#[test]
fn an_unexcused_late_wakeup_is_recorded_without_alerting_below_the_threshold() {
    let mut scenario = held_scenario();
    tick_with_lateness(&mut scenario, &[45_000]);

    assert_eq!(
        scenario.state().lifecycle.late_wakeups,
        std::collections::VecDeque::from([45_000])
    );
    assert_eq!(missed_alerts(&scenario), 0);
}

// CORE-002: a single late wakeup greater than 2 minutes alerts.
#[test]
fn a_single_late_wakeup_alerts_only_when_it_is_over_two_minutes() {
    for (late, alerts) in [(120_000, 0), (120_001, 1)] {
        let mut scenario = held_scenario();
        tick_with_lateness(&mut scenario, &[late]);
        assert_eq!(
            missed_alerts(&scenario),
            alerts,
            "{late} ms late should produce {alerts} alerts"
        );
    }
}

// CORE-002: the sum of the last 10 wakeups greater than 5 minutes alerts; the array keeps only the last 10.
#[test]
fn the_sum_of_recent_late_wakeups_alerts_only_when_it_is_over_five_minutes() {
    struct Series {
        name: &'static str,
        lateness: Vec<i64>,
        alerts: usize,
    }
    let series = [
        Series {
            name: "ten wakeups summing to exactly 5 minutes do not alert",
            lateness: vec![30_000; 10],
            alerts: 0,
        },
        Series {
            name: "ten wakeups summing to 5 minutes and 10 ms alert",
            lateness: vec![30_001; 10],
            alerts: 1,
        },
        Series {
            name: "wakeups older than the last 10 no longer count towards the sum",
            lateness: vec![30_000; 12],
            alerts: 0,
        },
        Series {
            name: "early wakeups are left out of the sum rather than offsetting it",
            lateness: vec![100_000, 100_000, -20_000, 100_000, 1_000],
            alerts: 1,
        },
        Series {
            name: "early wakeups do not count towards the sum",
            lateness: vec![100_000, 100_000, -20_000, 100_000],
            alerts: 0,
        },
    ];

    for case in &series {
        let mut scenario = held_scenario();
        tick_with_lateness(&mut scenario, &case.lateness);
        assert_eq!(missed_alerts(&scenario), case.alerts, "{}", case.name);
    }
}

// CORE-002: the late wakeups array tracks only the last 10 wakeups.
#[test]
fn the_late_wakeups_array_keeps_only_the_last_ten() {
    let mut scenario = held_scenario();
    let lateness: Vec<i64> = (1..=15).map(|i| i * 1_000).collect();
    tick_with_lateness(&mut scenario, &lateness);

    let kept: Vec<i64> = scenario
        .state()
        .lifecycle
        .late_wakeups
        .iter()
        .copied()
        .collect();
    assert_eq!(
        kept,
        (6..=15).map(|i| i * 1_000).collect::<Vec<i64>>(),
        "the oldest five entries should have been dropped"
    );
    assert_eq!(missed_alerts(&scenario), 0);
}

// CORE-002: the array is cleared after an alert, so the same incident does not alert twice.
#[test]
fn the_late_wakeups_array_is_cleared_after_an_alert_so_it_does_not_alert_twice() {
    let mut scenario = held_scenario();
    tick_with_lateness(&mut scenario, &[31_000; 10]);
    assert_eq!(
        missed_alerts(&scenario),
        1,
        "the tenth entry crosses 5 minutes"
    );
    assert!(scenario.state().lifecycle.late_wakeups.is_empty());

    // Another slightly late wakeup is a fresh count, not a repeat of the alert.
    tick_with_lateness(&mut scenario, &[31_000]);
    assert_eq!(missed_alerts(&scenario), 1);
    assert_eq!(scenario.state().lifecycle.late_wakeups.len(), 1);

    // A new incident alerts again.
    tick_with_lateness(&mut scenario, &[130_000]);
    assert_eq!(missed_alerts(&scenario), 2);
}

// CORE-002: the late wakeup event is called screenshot_missed.
#[test]
fn the_late_wakeup_alert_is_a_screenshot_missed_event() {
    let mut scenario = held_scenario();
    tick_with_lateness(&mut scenario, &[600_000]);

    let kinds: Vec<String> = queued_events(&scenario)
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(
        kinds.iter().filter(|k| *k == "screenshot_missed").count(),
        1,
        "queued events: {kinds:?}"
    );
}

// CORE-002: tamper detection is skipped, recording nothing, where lifecycle is disabled.
#[test]
fn a_platform_without_lifecycle_signals_records_and_alerts_on_nothing() {
    let mut scenario = Scenario::authenticated();
    hold_batches(&mut scenario);
    scenario.platform.set_lifecycle_enabled(false);
    scenario.at_t(T0).tick();

    tick_with_lateness(&mut scenario, &[600_000, 600_000, 3_600_000]);

    assert!(scenario.state().lifecycle.late_wakeups.is_empty());
    assert_eq!(missed_alerts(&scenario), 0);
}

// ── A user-initiated stop excuses the gap that follows it ────────────────────

/// Restarts the daemon from `previous`'s state directory at `now_ms`, with
/// batches held back. `previous` must stay alive: dropping a `Scenario`
/// deletes its state directory.
fn restart(previous: &Scenario, now_ms: i64) -> Scenario {
    let mut next =
        Scenario::authenticated_with_state_dir_at(previous.state_dir_path().to_path_buf(), now_ms);
    hold_batches(&mut next);
    next
}

// CORE-002: a user stop excuses the first tick after the daemon actually restarts, and only that one.
#[test]
fn a_user_stop_excuses_only_the_first_tick_after_the_restart() {
    let mut first = held_scenario();
    first.note_user_stop("test");

    // Ticks in the still-running session are not the gap being excused, and
    // they record nothing even when very late.
    hold_batches(&mut first);
    first.at_t(T0 + 1_000).tick();
    hold_batches(&mut first);
    tick_with_lateness(&mut first, &[3_600_000]);
    assert!(first.state().lifecycle.late_wakeups.is_empty());
    assert_eq!(missed_alerts(&first), 0);

    let mut second = restart(&first, T0 + 2 * 3_600_000);
    second.tick();
    assert!(second.state().lifecycle.late_wakeups.is_empty());
    assert_eq!(missed_alerts(&second), 0, "the restart gap is excused");

    // The excuse is spent: the next late wakeup is reported as usual.
    tick_with_lateness(&mut second, &[600_000]);
    assert_eq!(missed_alerts(&second), 1);
}

// CORE-002: a process that merely exited (killed, or a caught termination signal) is not excused.
#[test]
fn a_clean_exit_without_a_user_stop_does_not_excuse_the_restart_gap() {
    for exit_with_request_stop in [false, true] {
        let first = held_scenario();
        if exit_with_request_stop {
            // The platform's signal handler asks the loop to stop and the
            // process exits cleanly.
            std::thread::scope(|scope| {
                let handle = scope.spawn(|| first.daemon.run_forever());
                first.daemon.request_stop();
                handle.join().expect("loop must stop cleanly");
            });
        }

        let mut second = restart(&first, T0 + 3_600_000);
        second.tick();
        assert_eq!(
            missed_alerts(&second),
            1,
            "an hour of downtime after a plain exit (request_stop: {exit_with_request_stop}) must be reported"
        );
    }
}

// ── Repeated restart detection (CORE-019) ────────────────────────────────────

fn has_repeated_restarts_alert(scenario: &Scenario) -> bool {
    scenario
        .api
        .state()
        .notify_calls
        .iter()
        .any(|call| call.payload.event_type == "repeated_restarts")
}

/// Starts the daemon once per entry of `times_ms` (the first must be 0) and
/// returns every instance, the last one still to be ticked. Each earlier
/// instance first runs `before_restart`.
fn restart_at(times_ms: &[i64], before_restart: impl Fn(&mut Scenario)) -> Vec<Scenario> {
    let mut scenarios = vec![Scenario::authenticated()];
    let state_dir = scenarios[0].state_dir_path().to_path_buf();
    for &time in &times_ms[1..] {
        let previous = scenarios.last_mut().unwrap();
        before_restart(previous);
        scenarios.push(Scenario::authenticated_with_state_dir_at(
            state_dir.clone(),
            time,
        ));
    }
    scenarios
}

/// Twenty restarts, the first at 0, the last `span_ms` later, the rest evenly between.
fn twenty_restarts_spanning(span_ms: i64) -> Vec<i64> {
    (0..20).map(|i| i * span_ms / 19).collect()
}

// CORE-019: 20 restarts inside the rolling 10-minute window alert; older ones are pruned.
#[test]
fn only_restarts_inside_the_ten_minute_window_count_towards_the_alert() {
    for (span_ms, alerts) in [(599_000, true), (601_000, false)] {
        let times = twenty_restarts_spanning(span_ms);
        let mut scenarios = restart_at(&times, |_| {});
        let last = scenarios.last_mut().unwrap();
        last.at_t(span_ms).tick();
        assert_eq!(
            has_repeated_restarts_alert(last),
            alerts,
            "20 restarts spanning {span_ms} ms"
        );
    }
}

// CORE-019: the alert fires regardless of user-stop status.
#[test]
fn restarts_after_a_user_stop_still_count_towards_the_alert() {
    let times = twenty_restarts_spanning(190_000);
    let mut scenarios = restart_at(&times, |previous| {
        previous.note_user_stop("test");
    });
    // Every earlier stop and resume left events ahead of the alert in the hash
    // queue, and a tick hashes at most 8 of them, so let the queue drain.
    let last = scenarios.last_mut().unwrap();
    for i in 0..12 {
        last.at_t(190_000 + i * 61_000).tick();
    }
    assert!(has_repeated_restarts_alert(last));
}

// CORE-019: the timestamps are cleared when the threshold is reached, alert sent or not.
#[test]
fn the_restart_timestamps_are_cleared_even_when_the_cooldown_suppresses_the_alert() {
    let first_burst = twenty_restarts_spanning(190_000);
    let mut scenarios = restart_at(&first_burst, |_| {});
    scenarios.last_mut().unwrap().at_t(190_000).tick();
    assert!(has_repeated_restarts_alert(scenarios.last().unwrap()));
    assert!(
        scenarios
            .last()
            .unwrap()
            .state()
            .lifecycle
            .restart_times
            .is_empty()
    );

    // A second burst a few minutes later, inside the 30-minute cooldown.
    let state_dir = scenarios[0].state_dir_path().to_path_buf();
    let base = 190_000 + 5 * 60_000;
    for i in 0..20 {
        scenarios.push(Scenario::authenticated_with_state_dir_at(
            state_dir.clone(),
            base + i * 10_000,
        ));
    }
    let last = scenarios.last_mut().unwrap();
    last.at_t(base + 190_000).tick();
    assert!(
        !has_repeated_restarts_alert(last),
        "suppressed by the cooldown"
    );
    assert!(last.state().lifecycle.restart_times.is_empty());
}

// CORE-019: the check is skipped where lifecycle is disabled.
#[test]
fn restarts_are_not_tracked_where_lifecycle_is_disabled() {
    let mut scenarios = vec![Scenario::authenticated()];
    let state_dir = scenarios[0].state_dir_path().to_path_buf();
    for i in 1..25 {
        scenarios.push(Scenario::authenticated_with_state_dir_configured(
            state_dir.clone(),
            i * 10_000,
            |platform, _| platform.set_lifecycle_enabled(false),
        ));
    }
    let last = scenarios.last_mut().unwrap();
    last.at_t(24 * 10_000).tick();

    assert!(!has_repeated_restarts_alert(last));
    assert!(
        last.state().lifecycle.restart_times.len() == 1,
        "only the first instance (lifecycle enabled) recorded a restart"
    );
}
