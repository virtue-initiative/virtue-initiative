//! The daemon's own clock (CORE-027), driven through the `Scenario` DSL: the
//! whole daemon with a fake clock, a fake server and fake platform hooks.
//!
//! Run with:
//!   cargo test -p virtue-core --features testing --test clock
#![cfg(feature = "testing")]

mod common;

use common::*;
use virtue_core::testing::{MockApiClient, TestPlatformHooks};

const HOUR_MS: i64 = 3_600_000;

// CORE-027: setting the system clock while the daemon runs changes nothing.
#[test]
fn setting_the_system_clock_while_running_changes_nothing() {
    let mut scenario = held_scenario();
    // No server to correct anything: only the daemon's own clock counts.
    scenario.api.serve_no_time();
    for offset_ms in [-DAY_MS, DAY_MS] {
        scenario.platform.set_system_clock_offset(offset_ms);
        let captures = scenario.platform.take_call_count();
        for _ in 0..20 {
            let next = wakeup(&scenario);
            assert!(next - scenario.clock.now_ms() <= DAY_MS);
            scenario.at_t(next).tick();
            assert_eq!(
                scenario.state().last_tick_at_ms,
                Some(scenario.clock.now_ms())
            );
        }
        assert!(scenario.platform.take_call_count() > captures);
    }
    assert_eq!(missed_alerts(&scenario), 0);
    assert!(alert_messages(&scenario).is_empty());
}

/// A `restart_with` configuration: the server can't be reached, and the
/// system clock is `offset_ms` off.
fn offline_with_clock_off(offset_ms: i64) -> impl FnOnce(&TestPlatformHooks, &MockApiClient) {
    move |platform, api| {
        offline(platform, api);
        platform.set_system_clock_offset(offset_ms);
    }
}

// CORE-027: after an offline restart, the clock starts no earlier than the last tick; the server corrects it and the correction is reported.
#[test]
fn a_clock_set_back_before_an_offline_restart_is_corrected_and_reported() {
    let first = held_scenario();
    let start = T0 + HOUR_MS;
    let mut second = restart_with(&first, start, offline_with_clock_off(-2 * HOUR_MS));
    second.tick();
    assert_eq!(
        second.state().last_tick_at_ms,
        Some(T0),
        "no earlier than the last tick"
    );

    reach_server(&mut second);
    assert_eq!(second.state().last_tick_at_ms, Some(start));
    assert_eq!(
        alert_messages(&second),
        [
            "This device's clock was 1 hour behind until it reached the server, so times it recorded before then may be off by that much."
        ]
    );
}

// CORE-027: a correction moves scheduled work with it, so a clock that was ahead holds nothing off.
#[test]
fn a_correction_moves_scheduled_work_with_it() {
    let first = held_scenario();
    let start = T0 + 30_000;
    let mut second = restart_with(&first, start, offline_with_clock_off(DAY_MS));
    second.tick();
    assert!(wakeup(&second) > second.clock.now_ms() + DAY_MS / 2);

    reach_server(&mut second);
    let now = second.clock.now_ms();
    let state = second.state();
    assert!(
        wakeup(&second) < now + DAY_MS / 2,
        "nothing is held off a day"
    );
    assert!(state.screenshot.next_screenshot_at_ms.unwrap() < now + DAY_MS / 2);
    assert!(state.heartbeat.last_heartbeat_ms <= now);
    assert!(state.lifecycle.last_tick_utc_ms.unwrap() <= now);
    assert_eq!(alert_messages(&second).len(), 1);
    assert!(alert_messages(&second)[0].contains("was 1 day ahead"));
}

// CORE-027: a correction made while starting moves nothing and is not reported.
#[test]
fn a_correction_while_starting_is_not_reported() {
    let first = held_scenario();
    let start = T0 + 30_000;
    let mut second = restart_with(&first, start, |platform, _| {
        platform.set_system_clock_offset(-DAY_MS);
    });
    second.tick();
    assert_eq!(second.state().last_tick_at_ms, Some(start));
    assert!(alert_messages(&second).is_empty());
}

// CORE-027: a correction of 5 minutes or less is not reported.
#[test]
fn a_correction_of_five_minutes_or_less_is_not_reported() {
    let first = held_scenario();
    let start = T0 + HOUR_MS;
    let mut second = restart_with(&first, start, offline_with_clock_off(-5 * 60_000));
    second.tick();
    reach_server(&mut second);
    assert!(alert_messages(&second).is_empty());
    assert_eq!(second.state().last_tick_at_ms, Some(start));
}

// CORE-027: no correction is reported where lifecycle is disabled.
#[test]
fn a_correction_is_not_reported_where_lifecycle_is_disabled() {
    let first = held_scenario();
    let start = T0 + HOUR_MS;
    let mut second = restart_with(&first, start, |platform, api| {
        offline_with_clock_off(-2 * HOUR_MS)(platform, api);
        platform.set_lifecycle_enabled(false);
    });
    second.tick();
    reach_server(&mut second);
    assert!(alert_messages(&second).is_empty());
    assert_eq!(second.state().last_tick_at_ms, Some(start));
}
