use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::model::{AlertReason, UploadKind};
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
}

/// Phase 1 of `Daemon::tick_once`: compares `now_ms` to the wakeup time this
/// tick was scheduled for (the daemon's `next_wakeup_at_ms` as of the end of
/// the previous tick) and records how late the daemon woke, unless excused by
/// proximity to a system login/logout. Alerts via [`upload::enqueue`] once the
/// late-wakeup budget is crossed. See `client/core/SPEC.md` §2.
///
/// `expected_wakeup_at_ms == 0` means no wakeup has ever been scheduled yet
/// (the daemon's very first tick) — nothing to compare against.
pub fn tick(
    state: &mut LifecycleState,
    upload: &mut UploadState,
    hooks: &dyn LifecycleHooks,
    now_ms: i64,
    expected_wakeup_at_ms: i64,
) {
    if expected_wakeup_at_ms == 0 {
        return;
    }

    let near_login = hooks
        .get_last_login_utc_ms()
        .ok()
        .flatten()
        .is_some_and(|login_ms| (now_ms - login_ms).abs() <= LOGIN_LOGOUT_EXCUSE_MS);
    let near_logout = hooks
        .get_last_logout_utc_ms()
        .ok()
        .flatten()
        .is_some_and(|logout_ms| {
            (expected_wakeup_at_ms - logout_ms).abs() <= LOGIN_LOGOUT_EXCUSE_MS
        });
    if near_login || near_logout {
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
            UploadKind::LifecycleAlert {
                reason: AlertReason::LateWakeup,
            },
        );
    }
}

/// Handles an explicit user-initiated stop — an immediate high-risk alert,
/// independent of the late-wakeup model above. Called directly by
/// `Daemon::note_user_stop`.
pub fn note_user_stop(upload: &mut UploadState, now_ms: i64, source: &str) {
    tracing::info!(source, "user-initiated stop");
    upload::enqueue(
        upload,
        now_ms,
        EXTRA_HIGH_RISK,
        UploadKind::LifecycleAlert {
            reason: AlertReason::UserStop,
        },
    );
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
        upload.pending_hash_events.iter().any(|e| {
            matches!(
                e.event,
                UploadKind::LifecycleAlert {
                    reason: AlertReason::LateWakeup
                }
            )
        })
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
        let mut upload = upload_with_credentials();
        note_user_stop(&mut upload, 1_000, "test");
        let entry = upload
            .pending_hash_events
            .iter()
            .find(|e| {
                matches!(
                    e.event,
                    UploadKind::LifecycleAlert {
                        reason: AlertReason::UserStop
                    }
                )
            })
            .expect("expected a UserStop alert");
        assert!(entry.risk.unwrap() >= EXTRA_HIGH_RISK);
        assert!(
            upload.force_flush,
            "extra-high-risk upload should force an immediate flush"
        );
    }
}
