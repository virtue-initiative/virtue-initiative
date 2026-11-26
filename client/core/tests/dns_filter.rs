//! DNS filter check (CORE-029), driven through the `Scenario` DSL with
//! scripted DNS answers and TLS handshakes.
//!
//! Run with:
//!   cargo test -p virtue-core --features testing --test dns_filter
#![cfg(feature = "testing")]

mod common;

use common::*;
use virtue_core::testing::Scenario;

/// The control domains, in the order a check tries them.
const CONTROL_DOMAINS: [&str; 5] = [
    "virtueinitiative.org",
    "apple.com",
    "microsoft.com",
    "google.com",
    "one.one.one.one",
];
const CONTROL: &str = CONTROL_DOMAINS[0];
const DOMAIN: &str = "blocked.example";
const OTHER_DOMAIN: &str = "other.example";
const PUBLIC_IP: &str = "192.0.2.10";
const CONTROL_IP: &str = "192.0.2.20";
/// An address OpenDNS answers blocked names with: its block page, which shows
/// no valid certificate for the blocked name.
const BLOCK_PAGE_IP: &str = "146.112.61.104";
const MINUTE_MS: i64 = 60 * 1_000;
/// The longest delay between checks, so a tick this long after a check always
/// finds the next one due.
const HOUR_MS: i64 = 60 * MINUTE_MS;
const RETRY_MS: i64 = 10 * 60 * 1_000;
/// The time of the first check in [`checking_scenario`].
const T1: i64 = T0 + 1_000;

/// How a scripted host answers.
#[derive(Clone, Copy, Debug)]
enum Answer {
    Addrs(&'static [&'static str]),
    Fails,
}

use Answer::*;

const BLOCKED: Answer = Addrs(&["0.0.0.0"]);
const RESOLVES: Answer = Addrs(&[PUBLIC_IP]);
const CONTROL_RESOLVES: Answer = Addrs(&[CONTROL_IP]);

fn script(scenario: &Scenario, host: &str, answer: Answer) {
    match answer {
        Addrs(addrs) => scenario.dns.answer(host, addrs),
        Fails => scenario.dns.fail(host),
    }
}

/// An authenticated scenario with a reachable first control domain and
/// `DOMAIN` set as the check domain. No check has run yet. The other control
/// domains do not resolve, so they matter only once the first one fails.
///
/// The screen is locked, which holds uploads back (CORE-005) even for the
/// extra-high-risk DNS filter alerts, so the events the daemon queues stay
/// visible until [`deliver`] unlocks it.
fn checking_scenario() -> Scenario {
    let mut scenario = held_scenario();
    scenario.platform.set_locked_or_screensaver(true);
    script(&scenario, CONTROL, CONTROL_RESOLVES);
    scenario.set_dns_filter_domain(Some(DOMAIN));
    scenario
}

/// Unlocks the screen and ticks at `at_ms`, letting the queued events upload.
fn deliver(scenario: &mut Scenario, at_ms: i64) {
    scenario.platform.set_locked_or_screensaver(false);
    scenario.at_t(at_ms).tick();
}

/// The DNS filter events that reached the API as partner alert emails
/// (API-038 `notifications`), as `(type, risk)`.
fn notified_dns(scenario: &Scenario) -> Vec<(String, f32)> {
    scenario
        .api
        .state()
        .notify_calls
        .iter()
        .filter(|call| call.payload.event_type.starts_with("dns_filter"))
        .map(|call| (call.payload.event_type.clone(), call.payload.risk))
        .collect()
}

/// The DNS filter events waiting in the upload queues, oldest first.
fn dns_events(scenario: &Scenario) -> Vec<QueuedEvent> {
    queued_events(scenario)
        .into_iter()
        .filter(|e| e.kind.starts_with("dns_filter"))
        .collect()
}

fn dns_event_kinds(scenario: &Scenario) -> Vec<String> {
    dns_events(scenario).into_iter().map(|e| e.kind).collect()
}

/// How often `host` has been looked up.
fn lookups_of(scenario: &Scenario, host: &str) -> usize {
    scenario
        .dns
        .lookups()
        .iter()
        .filter(|h| h.as_str() == host)
        .count()
}

/// The addresses a TLS handshake was tried with for `host`, in order.
fn handshakes_with(scenario: &Scenario, host: &str) -> Vec<String> {
    scenario
        .dns
        .handshakes()
        .into_iter()
        .filter(|(h, _)| h == host)
        .map(|(_, addr)| addr.to_string())
        .collect()
}

/// The control domains looked up so far, in order.
fn control_lookups(scenario: &Scenario) -> Vec<String> {
    scenario
        .dns
        .lookups()
        .into_iter()
        .filter(|h| CONTROL_DOMAINS.contains(&h.as_str()))
        .collect()
}

/// Keeps the screenshot draw from deciding the next wakeup, so it shows the
/// DNS filter check's own schedule.
fn push_screenshot_out(scenario: &mut Scenario) {
    scenario.with_state_mut(|s| s.screenshot.next_screenshot_at_ms = Some(FAR_FUTURE));
}

// ── The first result ──────────────────────────────────────────────────────────

// CORE-029, CORE-005: a blocked answer (failed, unspecified or loopback only) is filtered and
// stays quiet on the first check; anything else is unfiltered and alerts partners at once.
#[test]
fn the_first_check_alerts_only_when_the_domain_resolves_normally() {
    let cases: &[(&str, Answer, bool)] = &[
        ("lookup fails", Fails, false),
        ("unspecified v4", Addrs(&["0.0.0.0"]), false),
        ("unspecified v6", Addrs(&["::"]), false),
        ("loopback v4", Addrs(&["127.0.0.1"]), false),
        ("loopback v6", Addrs(&["::1"]), false),
        ("empty answer", Addrs(&[]), false),
        ("blocked v4 and v6", Addrs(&["0.0.0.0", "::"]), false),
        ("public v4", Addrs(&[PUBLIC_IP]), true),
        ("public v6", Addrs(&["2001:db8::1"]), true),
        ("public among blocked", Addrs(&["0.0.0.0", PUBLIC_IP]), true),
    ];
    for &(name, answer, alerts) in cases {
        let mut scenario = checking_scenario();
        script(&scenario, DOMAIN, answer);
        scenario.at_t(T1).tick();

        assert_eq!(lookups_of(&scenario, DOMAIN), 1, "{name}: one check");
        let events = dns_events(&scenario);
        if alerts {
            assert_eq!(events.len(), 1, "{name}: {events:?}");
            assert_eq!(events[0].kind, "dns_filter_inactive", "{name}");
            assert_eq!(events[0].domain.as_deref(), Some(DOMAIN), "{name}");
            assert_eq!(events[0].risk, Some(0.9), "{name}: risk 90%");
        } else {
            assert!(events.is_empty(), "{name}: {events:?}");
        }

        // Once uploads can go out, the alert is emailed at once, without
        // waiting for the batch interval.
        deliver(&mut scenario, T1 + 1_000);
        assert!(dns_events(&scenario).is_empty(), "{name}: uploaded");
        let expected = if alerts {
            vec![("dns_filter_inactive".to_string(), 0.9)]
        } else {
            vec![]
        };
        assert_eq!(notified_dns(&scenario), expected, "{name}");
    }
}

// ── Transitions ───────────────────────────────────────────────────────────────

// CORE-029: events are sent only when the result changes.
#[test]
fn hourly_checks_report_only_changes_of_the_result() {
    let mut scenario = checking_scenario();
    let answers = [RESOLVES, RESOLVES, BLOCKED, BLOCKED, RESOLVES];
    for (i, answer) in answers.into_iter().enumerate() {
        script(&scenario, DOMAIN, answer);
        scenario.at_t(T1 + i as i64 * HOUR_MS).tick();
    }

    assert_eq!(lookups_of(&scenario, DOMAIN), answers.len());
    let events = dns_events(&scenario);
    let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "dns_filter_inactive",
            "dns_filter_active",
            "dns_filter_inactive"
        ]
    );
    assert_eq!(events[1].risk, Some(0.0), "dns_filter_active is risk 0%");
    assert!(events.iter().all(|e| e.domain.as_deref() == Some(DOMAIN)));
}

// CORE-029, CORE-020: each check draws a new delay, exponential with a mean
// of 20 minutes and clamped to 1 to 60 minutes, and the next check is a wakeup.
#[test]
fn each_check_draws_a_new_random_delay() {
    let mut scenario = checking_scenario();
    script(&scenario, DOMAIN, BLOCKED);
    // (uniform draw, expected delay): the clamp at both ends, the mean, and a
    // draw in between.
    let draws = [
        (0.0, MINUTE_MS),
        (1.0 - (-1.0f64).exp(), 20 * MINUTE_MS),
        (0.999, HOUR_MS),
        (0.5, (-(0.5f64).ln() * 20.0 * MINUTE_MS as f64) as i64),
    ];

    let mut check_at = T1;
    for (checks, (u, expected)) in (1..).zip(draws) {
        scenario.rng.set_default(u);
        scenario.at_t(check_at).tick();
        assert_eq!(
            lookups_of(&scenario, DOMAIN),
            checks,
            "checked at the drawn time"
        );
        push_screenshot_out(&mut scenario);

        let next = scenario.state().dns_filter.next_check_at_ms;
        assert!(
            (next - check_at - expected).abs() <= 1,
            "u = {u}: expected a delay of {expected} ms, got {} ms",
            next - check_at
        );
        assert!(wakeup(&scenario) <= next, "the next check is a wakeup");

        scenario.at_t(next - 1).tick();
        assert_eq!(
            lookups_of(&scenario, DOMAIN),
            checks,
            "not due before the drawn delay"
        );
        check_at = next;
    }
}

// ── Unknown results ───────────────────────────────────────────────────────────

// CORE-029, CORE-020: when no control domain is reachable the result is unknown:
// no event, the last result is kept, and the check is retried after 10 minutes.
// Only the first control domain resolves in these scenarios, so blocking it
// leaves none.
#[test]
fn a_blocked_control_domain_sends_nothing_and_retries_after_10_minutes() {
    let control_cases: &[(&str, Answer)] = &[
        ("lookup fails", Fails),
        ("unspecified", Addrs(&["0.0.0.0"])),
        ("loopback", Addrs(&["127.0.0.1"])),
    ];
    for &(name, control) in control_cases {
        let mut scenario = checking_scenario();
        script(&scenario, DOMAIN, RESOLVES);
        scenario.at_t(T1).tick();
        assert_eq!(
            dns_event_kinds(&scenario),
            ["dns_filter_inactive"],
            "{name}"
        );

        // The network breaks; the check domain answer must not count.
        script(&scenario, CONTROL, control);
        script(&scenario, DOMAIN, BLOCKED);
        let t2 = T1 + HOUR_MS;
        scenario.at_t(t2).tick();
        assert_eq!(lookups_of(&scenario, CONTROL), 2, "{name}: control checked");
        assert_eq!(
            dns_event_kinds(&scenario),
            ["dns_filter_inactive"],
            "{name}: unknown sends nothing"
        );
        push_screenshot_out(&mut scenario);
        scenario.at_t(t2 + 1).tick();
        assert_eq!(wakeup(&scenario), t2 + RETRY_MS, "{name}: retry wakeup");

        scenario.at_t(t2 + RETRY_MS - 1).tick();
        assert_eq!(lookups_of(&scenario, CONTROL), 2, "{name}: no early retry");

        // The network recovers and the domain still resolves: the remembered
        // result is unfiltered, so this is no change.
        script(&scenario, CONTROL, CONTROL_RESOLVES);
        script(&scenario, DOMAIN, RESOLVES);
        scenario.at_t(t2 + RETRY_MS).tick();
        assert_eq!(lookups_of(&scenario, CONTROL), 3, "{name}: retried");
        assert_eq!(
            dns_event_kinds(&scenario),
            ["dns_filter_inactive"],
            "{name}: last result kept across the unknown"
        );
    }
}

// CORE-029, CORE-020: behind a captive portal every name resolves to the portal,
// which shows no valid certificate. No control domain is reachable, so the
// result is unknown, after trying each control domain in order, and the check
// domain is not even looked up.
#[test]
fn a_captive_portal_is_unknown_after_trying_every_control_domain() {
    const PORTAL_IP: &str = "198.51.100.1";
    let mut scenario = checking_scenario();
    for host in CONTROL_DOMAINS.into_iter().chain([DOMAIN]) {
        script(&scenario, host, Addrs(&[PORTAL_IP]));
        scenario.dns.fail_tls(host);
    }
    scenario.at_t(T1).tick();

    assert_eq!(control_lookups(&scenario), CONTROL_DOMAINS);
    for host in CONTROL_DOMAINS {
        assert_eq!(handshakes_with(&scenario, host), [PORTAL_IP], "{host}");
    }
    assert_eq!(lookups_of(&scenario, DOMAIN), 0, "check domain not checked");
    assert!(dns_events(&scenario).is_empty(), "no alert");
    assert!(
        scenario.state().dns_filter.last_result.is_none(),
        "no result recorded"
    );
    push_screenshot_out(&mut scenario);
    scenario.at_t(T1 + 1).tick();
    assert_eq!(wakeup(&scenario), T1 + RETRY_MS, "retry wakeup");
}

// CORE-029: the control domains are tried in order, stopping at the first
// reachable one, so a filter that blocks some of them still gets checked.
#[test]
fn the_control_domains_are_tried_in_order_until_one_is_reachable() {
    // (name, how many leading control domains are unreachable, whether they
    // fail the handshake rather than being blocked)
    let cases: &[(&str, usize, bool)] = &[
        ("the first is reachable", 0, false),
        ("the first two are blocked", 2, false),
        ("all but the last are blocked", 4, false),
        ("the first three fail the handshake", 3, true),
    ];
    for &(name, unreachable, by_tls) in cases {
        let mut scenario = checking_scenario();
        for (i, host) in CONTROL_DOMAINS.into_iter().enumerate() {
            if i >= unreachable {
                script(&scenario, host, CONTROL_RESOLVES);
            } else if by_tls {
                script(&scenario, host, CONTROL_RESOLVES);
                scenario.dns.fail_tls(host);
            } else {
                script(&scenario, host, BLOCKED);
            }
        }
        script(&scenario, DOMAIN, RESOLVES);
        scenario.at_t(T1).tick();

        assert_eq!(
            control_lookups(&scenario),
            CONTROL_DOMAINS[..=unreachable],
            "{name}"
        );
        assert_eq!(
            dns_event_kinds(&scenario),
            ["dns_filter_inactive"],
            "{name}: checked"
        );
    }
}

// CORE-029: a filter that answers with the address of a block page is in use:
// the block page shows no valid certificate for the check domain.
#[test]
fn a_block_page_counts_as_filtered() {
    let mut scenario = checking_scenario();
    script(&scenario, DOMAIN, Addrs(&[BLOCK_PAGE_IP]));
    scenario.dns.fail_tls(DOMAIN);
    scenario.at_t(T1).tick();
    assert_eq!(handshakes_with(&scenario, DOMAIN), [BLOCK_PAGE_IP]);
    assert!(
        dns_events(&scenario).is_empty(),
        "a first filtered is quiet"
    );

    // The filter is turned off: the real site answers with its certificate.
    script(&scenario, DOMAIN, RESOLVES);
    scenario.dns.serve_tls(DOMAIN);
    scenario.at_t(T1 + HOUR_MS).tick();

    // And on again.
    script(&scenario, DOMAIN, Addrs(&[BLOCK_PAGE_IP]));
    scenario.dns.fail_tls(DOMAIN);
    scenario.at_t(T1 + 2 * HOUR_MS).tick();

    assert_eq!(
        dns_event_kinds(&scenario),
        ["dns_filter_inactive", "dns_filter_active"]
    );
}

// CORE-029: unspecified and loopback addresses are never connected to, and a
// blocked answer is filtered without any handshake.
#[test]
fn blocked_addresses_are_never_connected_to() {
    let cases: &[(&str, Answer, &[&str])] = &[
        ("blocked", Addrs(&["0.0.0.0", "::"]), &[]),
        (
            "public among blocked",
            Addrs(&["0.0.0.0", "127.0.0.1", PUBLIC_IP]),
            &[PUBLIC_IP],
        ),
    ];
    for &(name, answer, connected) in cases {
        let mut scenario = checking_scenario();
        script(&scenario, DOMAIN, answer);
        scenario.at_t(T1).tick();
        assert_eq!(lookups_of(&scenario, DOMAIN), 1, "{name}: checked");
        assert_eq!(handshakes_with(&scenario, DOMAIN), connected, "{name}");
    }
}

// ── Changing or removing the domain ───────────────────────────────────────────

// CORE-029: a changed domain is checked right away and its first result counts
// as a first result, with no event for the change itself.
#[test]
fn a_changed_domain_is_checked_promptly_and_starts_fresh() {
    let mut scenario = checking_scenario();
    script(&scenario, DOMAIN, RESOLVES);
    script(&scenario, OTHER_DOMAIN, RESOLVES);
    scenario.at_t(T1).tick();

    scenario.set_dns_filter_domain(Some(OTHER_DOMAIN));
    scenario.at_t(T1 + 1_000).tick();

    assert_eq!(lookups_of(&scenario, OTHER_DOMAIN), 1, "checked right away");
    let events = dns_events(&scenario);
    let summary: Vec<(&str, Option<&str>)> = events
        .iter()
        .map(|e| (e.kind.as_str(), e.domain.as_deref()))
        .collect();
    assert_eq!(
        summary,
        [
            ("dns_filter_inactive", Some(DOMAIN)),
            ("dns_filter_inactive", Some(OTHER_DOMAIN)),
        ]
    );
}

// CORE-029: removing the domain reports the domain that was checked at risk 80%,
// so partners, whom the server already emailed (API-020), get no second email
// from the device, and stops checking.
#[test]
fn removing_the_domain_reports_it_and_stops_checking() {
    let mut scenario = checking_scenario();
    script(&scenario, DOMAIN, BLOCKED);
    scenario.at_t(T1).tick();
    assert!(dns_events(&scenario).is_empty());

    scenario.set_dns_filter_domain(None);
    scenario.at_t(T1 + 1_000).tick();

    let events = dns_events(&scenario);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].kind, "dns_filter_check_removed");
    assert_eq!(events[0].domain.as_deref(), Some(DOMAIN));
    assert_eq!(events[0].risk, Some(0.8), "risk 80%");
    assert!(scenario.state().dns_filter.domain.is_none(), "state reset");

    scenario.at_t(T1 + 2 * HOUR_MS).tick();
    assert_eq!(
        lookups_of(&scenario, DOMAIN),
        1,
        "no checks without a domain"
    );
    assert_eq!(dns_events(&scenario).len(), 1, "removal reported once");

    deliver(&mut scenario, T1 + 2 * HOUR_MS + 1_000);
    assert!(
        notified_dns(&scenario).is_empty(),
        "no tamper-alert email on top of the server's"
    );
}

// CORE-029, CORE-020: a domain that arrives with a settings refresh makes the next
// wakeup immediate, so it is checked right away rather than at the next timer.
#[test]
fn a_domain_from_a_settings_refresh_is_checked_on_the_next_wakeup() {
    let mut scenario = held_scenario();
    script(&scenario, CONTROL, CONTROL_RESOLVES);
    script(&scenario, DOMAIN, RESOLVES);
    scenario
        .api
        .state()
        .default_device_settings
        .dns_filter_domain = Some(DOMAIN.into());

    // A batch upload refreshes the settings at the end of the tick.
    let batches_before = scenario.api.state().batch_uploads.len();
    scenario.queue_upload(dev("carry the batch"));
    scenario.flush_batch_now();
    scenario.at_t(T1).tick();
    assert_eq!(
        scenario.api.state().batch_uploads.len(),
        batches_before + 1,
        "batch uploaded"
    );
    assert_eq!(lookups_of(&scenario, DOMAIN), 0, "not checked yet");
    assert_eq!(wakeup(&scenario), T1, "the check is due now");

    // The screen is unlocked here, so the alert goes out in the same tick,
    // although the batch interval is held back.
    scenario.at_t(T1).tick();
    assert_eq!(lookups_of(&scenario, DOMAIN), 1);
    assert!(dns_events(&scenario).is_empty(), "uploaded at once");
    assert_eq!(
        notified_dns(&scenario),
        [("dns_filter_inactive".to_string(), 0.9)]
    );
}

// ── Authentication and persistence ────────────────────────────────────────────

// CORE-029, CORE-009: nothing is checked once logged out.
#[test]
fn nothing_is_checked_while_logged_out() {
    let mut scenario = checking_scenario();
    script(&scenario, DOMAIN, RESOLVES);
    scenario.logout().unwrap();

    scenario.at_t(T1).tick();
    scenario.at_t(T1 + HOUR_MS).tick();

    assert!(
        scenario.dns.lookups().is_empty(),
        "{:?}",
        scenario.dns.lookups()
    );
    assert!(dns_events(&scenario).is_empty());
}

// CORE-029, CORE-016: the last result survives a restart, so an unchanged result
// is not reported again.
#[test]
fn the_last_result_survives_a_restart() {
    let mut scenario1 = checking_scenario();
    script(&scenario1, DOMAIN, RESOLVES);
    scenario1.at_t(T1).tick();
    assert_eq!(dns_event_kinds(&scenario1), ["dns_filter_inactive"]);
    let state_dir = scenario1.state_dir_path().to_path_buf();

    let mut scenario2 = Scenario::authenticated_with_state_dir_configured(
        state_dir,
        T1 + HOUR_MS,
        |platform, api| {
            platform.set_locked_or_screensaver(true);
            api.state().default_device_settings.dns_filter_domain = Some(DOMAIN.into());
        },
    );
    script(&scenario2, CONTROL, CONTROL_RESOLVES);
    script(&scenario2, DOMAIN, RESOLVES);
    scenario2.at_t(T1 + HOUR_MS).tick();

    assert_eq!(
        lookups_of(&scenario2, DOMAIN),
        1,
        "checked after the restart"
    );
    assert_eq!(
        dns_event_kinds(&scenario2),
        ["dns_filter_inactive"],
        "only the alert from before the restart"
    );
    drop(scenario1);
}

// CORE-016, CORE-029: a state file written before the DNS filter check existed
// still loads, and the check starts from no result.
#[test]
fn a_state_file_without_the_dns_filter_key_still_loads() {
    let scenario1 = Scenario::authenticated();
    let state_dir = scenario1.state_dir_path().to_path_buf();
    let old_state = serde_json::json!({
        "version": 1,
        "auth": { "device_credentials": { "device_id": "d", "refresh_token": "r" } },
        "lifecycle": {},
        "screenshot": { "enabled": true },
        "upload": { "device_credentials": { "device_id": "d", "refresh_token": "r" } },
        "capture_availability": {},
        "heartbeat": { "last_heartbeat_ms": T0 },
        "next_wakeup_at_ms": 0,
    });
    std::fs::write(
        state_dir.join("event_state.json"),
        serde_json::to_vec(&old_state).unwrap(),
    )
    .unwrap();

    let mut scenario2 =
        Scenario::authenticated_with_state_dir_configured(state_dir, T0, |platform, api| {
            // Holds the alert back so it stays visible (CORE-005).
            platform.set_locked_or_screensaver(true);
            api.state().default_device_settings.dns_filter_domain = Some(DOMAIN.into());
        });
    let loaded = scenario2.state();
    assert_eq!(
        loaded.heartbeat.last_heartbeat_ms, T0,
        "the old file was read"
    );
    assert!(loaded.dns_filter.domain.is_none());
    assert!(loaded.dns_filter.last_result.is_none());

    script(&scenario2, CONTROL, CONTROL_RESOLVES);
    script(&scenario2, DOMAIN, RESOLVES);
    scenario2.at_t(T1).tick();
    assert_eq!(dns_event_kinds(&scenario2), ["dns_filter_inactive"]);
    drop(scenario1);
}
