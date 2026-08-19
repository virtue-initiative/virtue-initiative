use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::model::UploadKind;
use crate::module::upload::{self, UploadState};
use crate::platform::LifecycleHooks;

pub(crate) const EXTRA_HIGH_RISK: f32 = 0.9;
/// High-risk lifecycle alerts that are still noteworthy but don't warrant an
/// immediate notification. The upload module routes `risk >= EXTRA_HIGH_RISK`
/// through the immediate (emailed) path; keeping late-wakeup alerts just below
/// that threshold flags them as high for review/sorting while letting them
/// ride the normal batch.
pub(crate) const HIGH_RISK_LIFECYCLE_ALERT: f32 = 0.8;

/// See `client/core/SPEC.md` §2.
const MAX_TRACKED_WAKEUPS: usize = 10;
const SINGLE_LATE_THRESHOLD_MS: i64 = 60_000; // 1 minute
const SUM_LATE_THRESHOLD_MS: i64 = 5 * 60_000; // 5 minutes
const LOGIN_LOGOUT_EXCUSE_MS: i64 = 2 * 60_000; // 2 minutes

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct LifecycleState {
    /// Lateness (`actual - expected`, may be negative) of the last up to
    /// [`MAX_TRACKED_WAKEUPS`] non-excused wakeups, oldest first.
    pub late_wakeups: VecDeque<i64>,
    /// Last system login/logout time seen, so a change can be detected and
    /// reported once. See `SPEC.md` §5.
    pub last_seen_login_ms: Option<i64>,
    pub last_seen_logout_ms: Option<i64>,
    /// Set by `note_user_stop` (never by a plain clean shutdown — see that
    /// function's doc comment for why) and cleared by `note_user_start`.
    /// While true, `tick()` ignores lateness entirely — no gap is recorded,
    /// checked, or alerted on — so the gap the stop itself caused is never
    /// mistaken for tampering. See `SPEC.md` §2.
    pub monitoring_stopped: bool,
}

/// Phase 1 of `Daemon::run_phases`: compares `now_ms` to the wakeup time this
/// tick was scheduled for (the daemon's `next_wakeup_at_ms` as of the end of
/// the previous tick) and records how late the daemon woke, unless excused by
/// a login/logout bracket that isn't contradicted by the other side's
/// evidence (see the excuse logic below). Alerts via [`upload::enqueue`] once
/// the late-wakeup budget is crossed. See `client/core/SPEC.md` §2.
///
/// `expected_wakeup_at_ms == 0` means no wakeup has ever been scheduled yet
/// (the daemon's very first tick, or the first tick after `note_user_start`
/// reset the schedule) — nothing to compare against.
pub fn tick(
    state: &mut LifecycleState,
    upload: &mut UploadState,
    hooks: &dyn LifecycleHooks,
    now_ms: i64,
    expected_wakeup_at_ms: i64,
) {
    if expected_wakeup_at_ms == 0 || state.monitoring_stopped {
        return;
    }

    // Each side is `None` (no evidence either way), `Some(true)` (supports a
    // legitimate session transition), or `Some(false)` (contradicts one). A
    // gap is excused only if at least one side supports it and neither side
    // contradicts — a single contradicting signal (e.g. the daemon was
    // killed well before an eventual reboot, so `expected_wakeup_at_ms`
    // isn't actually near the logout) blocks the excuse even though the
    // other side looks fine on its own. See `SPEC.md` §2.
    let login_evidence = hooks
        .get_last_login_utc_ms()
        .ok()
        .flatten()
        .map(|login_ms| (now_ms - login_ms).abs() <= LOGIN_LOGOUT_EXCUSE_MS);
    let logout_evidence = hooks
        .get_last_logout_utc_ms()
        .ok()
        .flatten()
        .map(|logout_ms| (expected_wakeup_at_ms - logout_ms).abs() <= LOGIN_LOGOUT_EXCUSE_MS);

    let has_evidence = login_evidence.is_some() || logout_evidence.is_some();
    let contradicted = login_evidence == Some(false) || logout_evidence == Some(false);
    if has_evidence && !contradicted {
        return;
    }

    let diff_ms = now_ms - expected_wakeup_at_ms;
    state.late_wakeups.push_back(diff_ms);
    while state.late_wakeups.len() > MAX_TRACKED_WAKEUPS {
        state.late_wakeups.pop_front();
    }

    let max_single = state.late_wakeups.iter().copied().max().unwrap_or(0);
    let sum_nonneg: i64 = state.late_wakeups.iter().copied().filter(|&d| d > 0).sum();

    if max_single > SINGLE_LATE_THRESHOLD_MS || sum_nonneg > SUM_LATE_THRESHOLD_MS {
        upload::enqueue(
            upload,
            now_ms,
            HIGH_RISK_LIFECYCLE_ALERT,
            UploadKind::ScreenshotMissed,
        );
        // See `SPEC.md` §2: cleared so the same already-alerted-on lateness
        // doesn't still be sitting in the array to alert again on the very
        // next tick.
        state.late_wakeups.clear();
    }
}

/// Phase 1b of `Daemon::run_phases`: detects a change in the last known
/// system login/logout time since the previous tick and records a
/// zero-risk informational event each time one is seen. The very first
/// observation (no prior baseline) only seeds `last_seen_*` — it does not
/// count as a "change" and is not reported. See `client/core/SPEC.md` §5.
pub fn note_session_events(
    state: &mut LifecycleState,
    upload: &mut UploadState,
    hooks: &dyn LifecycleHooks,
    now_ms: i64,
) {
    if let Ok(Some(login_ms)) = hooks.get_last_login_utc_ms()
        && state.last_seen_login_ms != Some(login_ms)
    {
        let had_baseline = state.last_seen_login_ms.is_some();
        state.last_seen_login_ms = Some(login_ms);
        if had_baseline {
            upload::enqueue(
                upload,
                now_ms,
                0.0,
                UploadKind::SystemLogin { utc_ms: login_ms },
            );
        }
    }

    if let Ok(Some(logout_ms)) = hooks.get_last_logout_utc_ms()
        && state.last_seen_logout_ms != Some(logout_ms)
    {
        let had_baseline = state.last_seen_logout_ms.is_some();
        state.last_seen_logout_ms = Some(logout_ms);
        if had_baseline {
            upload::enqueue(
                upload,
                now_ms,
                0.0,
                UploadKind::SystemLogout { utc_ms: logout_ms },
            );
        }
    }
}

/// Handles an explicit user-initiated stop — an immediate high-risk alert,
/// independent of the late-wakeup model above. Called directly by
/// `Daemon::note_user_stop`.
///
/// This is the ONLY thing that suspends lateness checking
/// (`monitoring_stopped`) — deliberately not tied to a clean `request_stop`
/// shutdown in general, since every platform's signal handler also calls
/// `request_stop` on a plain SIGTERM/kill. Excusing on any clean exit would
/// let the simplest possible evasion (just kill the process) silently
/// defeat tamper detection; excusing only here means the gap is forgiven
/// exactly when — and only when — it was already reported via this alert.
/// Checking resumes only once `note_user_start` is called. See
/// `client/core/SPEC.md` §2.
pub fn note_user_stop(
    state: &mut LifecycleState,
    upload: &mut UploadState,
    now_ms: i64,
    source: &str,
) {
    tracing::info!(source, "user-initiated stop");
    upload::enqueue(upload, now_ms, EXTRA_HIGH_RISK, UploadKind::UserStop);
    state.monitoring_stopped = true;
}

/// Re-enables lifecycle tamper detection after a prior `note_user_stop`.
/// Called by `Daemon::new` for every fresh monitoring session (and,
/// redundantly but harmlessly, wherever a platform explicitly signals that
/// monitoring has resumed — see `Daemon::note_user_start`).
///
/// Returns whether a stop was actually active (i.e. whether this call did
/// anything). The caller uses that to decide whether to also reset the
/// wakeup schedule baseline: it must NOT be reset unconditionally on every
/// restart, since that would let a plain kill signal escape detection too
/// — see `SPEC.md` §2 and the `daemon.rs` caller.
pub fn note_user_start(state: &mut LifecycleState) -> bool {
    let was_stopped = state.monitoring_stopped;
    state.monitoring_stopped = false;
    state.late_wakeups.clear();
    was_stopped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::UploadKind;
    use crate::module::upload::UploadState;
    use crate::testing::TestPlatformHooks;

    #[allow(clippy::field_reassign_with_default)]
    fn upload_with_credentials() -> UploadState {
        let mut upload = UploadState::default();
        upload.device_credentials = Some(crate::model::DeviceCredentials {
            device_id: "d".into(),
            refresh_token: "r".into(),
        });
        upload
    }

    fn has_late_wakeup_alert(upload: &UploadState) -> bool {
        late_wakeup_alert_count(upload) > 0
    }

    fn late_wakeup_alert_count(upload: &UploadState) -> usize {
        upload
            .pending_hash_events
            .iter()
            .filter(|e| matches!(e.event, UploadKind::ScreenshotMissed))
            .count()
    }

    #[test]
    fn first_tick_with_no_scheduled_wakeup_is_a_noop() {
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        tick(&mut state, &mut upload, &hooks, 1_000, 0);
        assert!(state.late_wakeups.is_empty());
        assert!(!has_late_wakeup_alert(&upload));
    }

    #[test]
    fn small_lateness_is_tracked_but_does_not_alert() {
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        // 5s late — well under the 1-minute single-wakeup threshold.
        tick(&mut state, &mut upload, &hooks, 305_000, 300_000);
        assert_eq!(state.late_wakeups, [5_000]);
        assert!(!has_late_wakeup_alert(&upload));
    }

    #[test]
    fn single_wakeup_over_one_minute_late_alerts() {
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        tick(&mut state, &mut upload, &hooks, 361_001, 300_000);
        assert!(has_late_wakeup_alert(&upload));
    }

    #[test]
    fn accumulated_lateness_over_five_minutes_alerts() {
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        // Ten *recorded* wakeups each 31s late: sum = 310s > 300s (5 min), but
        // no single one exceeds the 1-minute per-wakeup threshold. The very
        // first tick (expected_wakeup_at_ms == 0) is a no-op by design, so
        // seed one throwaway tick first to get a nonzero `expected` baseline.
        let mut expected = 1_000i64;
        tick(&mut state, &mut upload, &hooks, expected, 0);
        for _ in 0..10 {
            let now = expected + 31_000;
            tick(&mut state, &mut upload, &hooks, now, expected);
            expected = now;
        }
        assert!(has_late_wakeup_alert(&upload));
    }

    #[test]
    fn negative_diffs_do_not_count_toward_the_sum_budget() {
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        // Early wakeups (negative diff) must not offset or contribute to the
        // non-negative sum in a way that fools the budget either direction.
        let mut expected = 0i64;
        for _ in 0..10 {
            let now = expected - 1_000; // 1s early each time
            tick(&mut state, &mut upload, &hooks, now, expected);
            expected = now;
        }
        assert!(!has_late_wakeup_alert(&upload));
    }

    #[test]
    fn wakeup_near_login_is_excused() {
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        // now_ms is within 2 minutes of a reported login — a huge lateness is
        // excused entirely (not even recorded).
        hooks.set_last_login(Some(360_500));
        tick(&mut state, &mut upload, &hooks, 361_000, 300_000);
        assert!(state.late_wakeups.is_empty());
        assert!(!has_late_wakeup_alert(&upload));
    }

    #[test]
    fn wakeup_near_logout_is_excused() {
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        // The *expected* wakeup time (not now_ms) is within 2 minutes of a
        // reported logout.
        hooks.set_last_logout(Some(301_000));
        tick(&mut state, &mut upload, &hooks, 500_000, 300_000);
        assert!(state.late_wakeups.is_empty());
        assert!(!has_late_wakeup_alert(&upload));
    }

    #[test]
    fn kill_then_reboot_still_alerts_despite_a_nearby_login() {
        // The daemon was killed well before the machine was actually
        // rebooted: `expected_wakeup_at_ms` (300_000, computed before the
        // kill) is nowhere near the reboot's logout (900_000), even though
        // the restart happens right after the reboot's login (now_ms is
        // close to 950_000). The contradicting logout evidence must block
        // the login excuse — this is the exact "kill it, then reboot to
        // dodge detection" gap the excuse model exists to catch.
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        hooks.set_last_logout(Some(900_000));
        hooks.set_last_login(Some(950_500));
        tick(&mut state, &mut upload, &hooks, 951_000, 300_000);
        // Counted (not excused) and alerted on — then cleared per SPEC.md §2.
        assert!(state.late_wakeups.is_empty());
        assert!(has_late_wakeup_alert(&upload));
    }

    #[test]
    fn never_restarted_after_reboot_still_alerts_despite_a_nearby_logout() {
        // Autostart is disabled: the machine rebooted (a real logout/login
        // pair fires right around the reboot) but the daemon itself isn't
        // manually started until long after. `expected_wakeup_at_ms`
        // (900_500) is close to the logout (900_000), but `now_ms`
        // (2_000_000) is nowhere near the login (950_000) that followed it.
        // The contradicting login evidence must block the logout excuse.
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        hooks.set_last_logout(Some(900_000));
        hooks.set_last_login(Some(950_000));
        tick(&mut state, &mut upload, &hooks, 2_000_000, 900_500);
        // Counted (not excused) and alerted on — then cleared per SPEC.md §2.
        assert!(state.late_wakeups.is_empty());
        assert!(has_late_wakeup_alert(&upload));
    }

    #[test]
    fn late_wakeups_array_is_cleared_after_an_alert_is_sent() {
        // SPEC.md §2: "The late wakeups array MUST be cleared after an alert
        // is sent (to prevent duplicates)."
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        tick(&mut state, &mut upload, &hooks, 361_001, 300_000);
        assert!(has_late_wakeup_alert(&upload));
        assert!(state.late_wakeups.is_empty());

        // Without the clear, the 61s-late entry above would still be sitting
        // in the array and this on-time-ish follow-up tick would alert a
        // second time for the same already-reported incident.
        tick(&mut state, &mut upload, &hooks, 361_501, 361_001);
        assert_eq!(
            late_wakeup_alert_count(&upload),
            1,
            "clearing after the alert must prevent a duplicate"
        );
    }

    #[test]
    fn accumulated_alert_does_not_duplicate_on_the_next_tick() {
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        let mut expected = 1_000i64;
        tick(&mut state, &mut upload, &hooks, expected, 0);
        for _ in 0..10 {
            let now = expected + 31_000;
            tick(&mut state, &mut upload, &hooks, now, expected);
            expected = now;
        }
        assert_eq!(late_wakeup_alert_count(&upload), 1);
        assert!(state.late_wakeups.is_empty());

        // Without clearing, 9 of the 10 contributing entries would still be
        // in the buffer and the sum would still be over budget, alerting
        // again for the same underlying incident on this next tick.
        let now = expected + 31_000;
        tick(&mut state, &mut upload, &hooks, now, expected);
        assert_eq!(
            late_wakeup_alert_count(&upload),
            1,
            "clearing after the alert must prevent a duplicate"
        );
    }

    #[test]
    fn late_wakeups_array_evicts_beyond_ten_entries() {
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        let mut expected = 1_000i64;
        for i in 0..15 {
            let now = expected + i; // tiny, harmless lateness
            tick(&mut state, &mut upload, &hooks, now, expected);
            expected = now;
        }
        assert_eq!(state.late_wakeups.len(), MAX_TRACKED_WAKEUPS);
    }

    #[test]
    fn user_stop_emits_immediate_extra_high_risk_alert() {
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        note_user_stop(&mut state, &mut upload, 1_000, "test");
        let entry = upload
            .pending_hash_events
            .iter()
            .find(|e| matches!(e.event, UploadKind::UserStop))
            .expect("expected a UserStop alert");
        assert!(entry.risk.unwrap() >= EXTRA_HIGH_RISK);
        assert!(
            upload.force_flush,
            "extra-high-risk upload should force an immediate flush"
        );
    }

    #[test]
    fn user_stop_suspends_lateness_checking_until_user_start() {
        // A kill-signal-triggered clean exit (SIGTERM etc.) must NOT excuse
        // anything on its own — only an actual user_stop alert should, since
        // that's the one case where the gap has already been reported.
        let mut state = LifecycleState::default();
        assert!(!state.monitoring_stopped);

        let mut upload = upload_with_credentials();
        note_user_stop(&mut state, &mut upload, 1_000, "test");
        assert!(state.monitoring_stopped);
    }

    // ── Other events (SPEC.md §5) ───────────────────────────────────────────

    fn system_event(upload: &UploadState, utc_ms: i64, login: bool) -> Option<f32> {
        upload
            .pending_hash_events
            .iter()
            .find_map(|e| match &e.event {
                UploadKind::SystemLogin { utc_ms: seen } if login && *seen == utc_ms => e.risk,
                UploadKind::SystemLogout { utc_ms: seen } if !login && *seen == utc_ms => e.risk,
                _ => None,
            })
    }

    #[test]
    fn first_observation_only_seeds_the_baseline_and_is_not_reported() {
        // SPEC.md §5: "The first System Login/Logout time observed [...]
        // MUST NOT be reported — it only establishes the baseline a later
        // change is measured against."
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        hooks.set_last_login(Some(1_000));
        hooks.set_last_logout(Some(2_000));

        note_session_events(&mut state, &mut upload, &hooks, 1_500);

        assert!(upload.pending_hash_events.is_empty());
        assert_eq!(state.last_seen_login_ms, Some(1_000));
        assert_eq!(state.last_seen_logout_ms, Some(2_000));
    }

    #[test]
    fn system_login_at_event_sent_when_login_time_changes_after_a_baseline_is_established() {
        // SPEC.md §5: "When the daemon detects that the System Login time
        // changed, it MUST send a "system login at" event (risk 0%)."
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        hooks.set_last_login(Some(1_000));
        note_session_events(&mut state, &mut upload, &hooks, 1_400); // seeds baseline, no event

        hooks.set_last_login(Some(9_000));
        note_session_events(&mut state, &mut upload, &hooks, 9_500);

        assert_eq!(system_event(&upload, 9_000, true), Some(0.0));
        assert_eq!(state.last_seen_login_ms, Some(9_000));
    }

    #[test]
    fn system_logout_at_event_sent_when_logout_time_changes_after_a_baseline_is_established() {
        // SPEC.md §5: "When the daemon detects tha[t] th[e] System Logout
        // time changed, it MUST send a "system logout at" event (risk 0%)."
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        hooks.set_last_logout(Some(2_000));
        note_session_events(&mut state, &mut upload, &hooks, 2_400); // seeds baseline, no event

        hooks.set_last_logout(Some(20_000));
        note_session_events(&mut state, &mut upload, &hooks, 20_500);

        assert_eq!(system_event(&upload, 20_000, false), Some(0.0));
        assert_eq!(state.last_seen_logout_ms, Some(20_000));
    }

    #[test]
    fn no_system_event_sent_when_login_and_logout_time_are_unchanged() {
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        hooks.set_last_login(Some(1_000));
        hooks.set_last_logout(Some(2_000));

        note_session_events(&mut state, &mut upload, &hooks, 1_500);
        let count_after_first = upload.pending_hash_events.len();
        note_session_events(&mut state, &mut upload, &hooks, 3_000);

        assert_eq!(
            upload.pending_hash_events.len(),
            count_after_first,
            "an unchanged login/logout time must not send a duplicate event"
        );
    }

    #[test]
    fn system_event_sent_again_when_login_time_changes_a_second_time() {
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();
        hooks.set_last_login(Some(1_000));
        note_session_events(&mut state, &mut upload, &hooks, 1_400); // seeds baseline, no event

        hooks.set_last_login(Some(5_000));
        note_session_events(&mut state, &mut upload, &hooks, 5_500);

        hooks.set_last_login(Some(9_000));
        note_session_events(&mut state, &mut upload, &hooks, 9_500);

        assert_eq!(system_event(&upload, 1_000, true), None);
        assert_eq!(system_event(&upload, 5_000, true), Some(0.0));
        assert_eq!(system_event(&upload, 9_000, true), Some(0.0));
        assert_eq!(state.last_seen_login_ms, Some(9_000));
    }

    // ── Intentional-stop excuse (SPEC.md §2) ────────────────────────────────

    #[test]
    fn ticks_before_the_daemon_actually_stops_do_not_consume_the_excuse() {
        // Regression test: `note_user_stop` and the eventual process exit
        // are two separate events, and the daemon can still tick in
        // between (e.g. servicing the `request_stop` that follows). Those
        // in-between ticks must not burn the excuse — only the real gap
        // caused by the daemon actually being down should be, and that
        // gap isn't visible until `note_user_start` runs on the next
        // session. See `SPEC.md` §2.
        let mut state = LifecycleState::default();
        let mut upload = upload_with_credentials();
        let hooks = TestPlatformHooks::new();

        note_user_stop(&mut state, &mut upload, 0, "test");
        assert!(state.monitoring_stopped);

        // A handful of ticks happen before the process actually exits (e.g.
        // while `request_stop` is being serviced), including one with a gap
        // that would otherwise alert. Under the old one-shot-flag design the
        // very first tick here would have wrongly burned the excuse.
        tick(&mut state, &mut upload, &hooks, 100, 50);
        tick(&mut state, &mut upload, &hooks, 10_000_000, 300_000);
        assert!(state.monitoring_stopped);
        assert!(state.late_wakeups.is_empty());
        assert!(!has_late_wakeup_alert(&upload));

        // Time passes while the process is down. Restarting: `note_user_start`
        // runs once per fresh session (mirroring `Daemon::new`). Resetting
        // the wakeup-schedule baseline so the first post-restart tick isn't
        // itself compared against a stale pre-stop schedule is `daemon.rs`'s
        // job (`apply_note_user_start`), exercised end-to-end in
        // `tests/scenarios.rs::user_stop_excuse_survives_across_a_real_restart_not_just_the_next_tick`.
        assert!(note_user_start(&mut state));
        assert!(!state.monitoring_stopped);

        // Checking is fully back to normal: a late wakeup is recorded again.
        tick(&mut state, &mut upload, &hooks, 20_305_000, 20_300_000);
        assert_eq!(state.late_wakeups.len(), 1);
    }

    #[test]
    fn note_user_start_is_a_no_op_when_not_stopped() {
        // A plain kill (no `note_user_stop`) must not be excused — asserted
        // more fully in `daemon.rs`'s `apply_note_user_start`, but at this
        // layer `note_user_start` must at least report there was nothing to
        // resume, so the caller knows not to reset the wakeup schedule.
        let mut state = LifecycleState::default();
        assert!(!note_user_start(&mut state));
        assert!(!state.monitoring_stopped);
    }
}
