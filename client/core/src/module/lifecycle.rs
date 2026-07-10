use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::Ping;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};
use crate::model::{AlertReason, LifecycleKind, PartialStatus, ProcessStoppedReason, UploadKind};
use crate::module::status::StatusRequest;
use crate::module::upload::Upload;
use crate::platform::LifecycleHooks;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessStarted;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessStopped(pub ProcessStoppedReason);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserStopRequested {
    pub source: String,
}

pub(crate) const EXTRA_HIGH_RISK: f32 = 0.9;
/// High-risk lifecycle alerts that are still noteworthy but don't warrant an
/// immediate notification. The upload module routes `risk >= EXTRA_HIGH_RISK`
/// through the immediate (emailed) path; keeping these just below that threshold
/// flags them as high for review/sorting while letting them ride the normal
/// batch.
pub(crate) const HIGH_RISK_LIFECYCLE_ALERT: f32 = 0.8;

// Sliding-window gap-budget detection, shared by all three gap buckets below.
// A single stall can be a blip (heavy capture/classify cycle, a slow poll) —
// alerting on one is twitchy, so we sum the time lost to over-threshold gaps
// inside a sliding window and only alert when that sustained gap budget is
// exceeded.
const PER_GAP_THRESHOLD_MS: i64 = 10_000; // a gap must exceed 10s to count
const GAP_WINDOW_MS: i64 = 10 * 60 * 1_000; // 10-min sliding window
const GAP_BUDGET_MS: i64 = 60_000; // alert when counted gap time >= 60s in window
const GAP_ALERT_COOLDOWN_MS: i64 = 5 * 60 * 1_000; // <= one alert per 5 min

const SUSPEND_MIN_MS: i64 = 5_000; // boot-vs-monotonic divergence worth logging
const LOGIN_POLL_INTERVAL_MS: i64 = 5 * 60 * 1_000; // coarse poll cadence while running

/// A single heartbeat's clock readings, used to compute the delta to the next
/// heartbeat.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default)]
pub struct HeartbeatSample {
    pub utc_ms: i64,
    pub boot_clock_ms: i64,
    pub monotonic_clock_ms: i64,
    pub login_id: u64,
}

/// Sliding-window gap-budget tracker shared by the three gap buckets
/// (unexpected gap / start / stop). Holds `(ts_ms, gap_ms)` for each gap that
/// exceeded the per-gap threshold; entries age out of the window.
/// `last_alert_ms` is the cooldown anchor (0 = never alerted).
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct GapTracker {
    pub gaps: Vec<(i64, i64)>,
    pub last_alert_ms: i64,
}

impl GapTracker {
    fn record(&mut self, now_ms: i64, gap_ms: i64) {
        self.gaps.push((now_ms, gap_ms));
    }

    /// Prune to the sliding window, sum the remaining gap time, and report
    /// whether the budget is newly crossed (respecting the cooldown). Gaps
    /// are intentionally NOT cleared on alert — chronic stalls keep
    /// re-alerting each cooldown, while one-off bursts age out of the window.
    fn crossed_budget(&mut self, now_ms: i64) -> bool {
        let window_start = now_ms - GAP_WINDOW_MS;
        self.gaps.retain(|(ts, _)| *ts >= window_start);
        let total: i64 = self.gaps.iter().map(|(_, gap)| *gap).sum();

        // The `== 0` short-circuit keeps the very first alert from being
        // suppressed by the cooldown (which is anchored at 0 = "never alerted").
        let cooldown_ok =
            self.last_alert_ms == 0 || now_ms - self.last_alert_ms >= GAP_ALERT_COOLDOWN_MS;

        if total >= GAP_BUDGET_MS && cooldown_ok {
            self.last_alert_ms = now_ms;
            true
        } else {
            false
        }
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct LifecycleObserverState {
    /// Bumped each time `get_last_login_utc_ms` reports a login newer than
    /// `last_login_utc_ms`. Tags each `HeartbeatSample` with the session it
    /// belongs to.
    pub login_id: u64,
    pub last_login_utc_ms: i64,
    pub last_logout_utc_ms: i64,

    /// Just the previous tick's clock readings — O(1), not a growing history.
    pub last_sample: Option<HeartbeatSample>,
    /// Last observed boot-clock value, used to detect a reboot (the
    /// boot-relative clocks resetting to a smaller value than previously seen).
    pub last_boot_clock_ms: i64,
    /// Throttles `get_last_login_utc_ms`/`get_last_logout_utc_ms`, which can
    /// be expensive (D-Bus round-trips, subprocess shell-outs) — polled at
    /// most every `LOGIN_POLL_INTERVAL_MS`, plus whenever `ProcessStarted`
    /// fires or a reboot is detected.
    pub last_login_poll_at_ms: i64,

    pub unexpected_gap: GapTracker,
    pub unexpected_start: GapTracker,
    pub unexpected_stop: GapTracker,
}

pub struct LifecycleModule {
    pub state: LifecycleObserverState,
    hooks: Box<dyn LifecycleHooks>,
}

impl LifecycleModule {
    pub fn new(hooks: Box<dyn LifecycleHooks>) -> Self {
        Self {
            state: LifecycleObserverState::default(),
            hooks,
        }
    }

    fn handle_ping(&mut self, emitter: &Emitter) -> CoreResult<()> {
        let utc_ms = self.hooks.get_utc_clock_ms()?;
        let boot_ms = self.hooks.get_boot_clock_ms()?;
        let mono_ms = self.hooks.get_monotonic_clock_ms()?;

        // A boot-clock value smaller than the last recorded one is the reboot
        // signal — the boot-relative clocks just reset, so a delta across
        // this boundary is meaningless; skip the mid-session math for this
        // tick and fall straight to the login/logout edge checks below,
        // anchored on UTC rather than the (now-reset) boot-relative clocks.
        let rebooted = self.state.last_boot_clock_ms > 0 && boot_ms < self.state.last_boot_clock_ms;

        if !rebooted && let Some(prev) = self.state.last_sample {
            self.evaluate_unexpected_gap(utc_ms, boot_ms, mono_ms, prev, emitter)?;
        }

        let should_poll = rebooted
            || self.state.last_login_poll_at_ms == 0
            || utc_ms - self.state.last_login_poll_at_ms >= LOGIN_POLL_INTERVAL_MS;
        if should_poll {
            self.poll_login_logout(utc_ms, boot_ms, mono_ms, emitter)?;
        }

        self.state.last_sample = Some(HeartbeatSample {
            utc_ms,
            boot_clock_ms: boot_ms,
            monotonic_clock_ms: mono_ms,
            login_id: self.state.login_id,
        });
        self.state.last_boot_clock_ms = boot_ms;

        Ok(())
    }

    /// Mid-session gap: awake time between two consecutive samples in the
    /// same boot that exceeds the per-gap threshold. The monotonic clock
    /// already excludes suspend, so the delta directly measures awake-but-
    /// unsampled time — crash, force-kill-and-restart, or a frozen process.
    fn evaluate_unexpected_gap(
        &mut self,
        utc_ms: i64,
        boot_ms: i64,
        mono_ms: i64,
        prev: HeartbeatSample,
        emitter: &Emitter,
    ) -> CoreResult<()> {
        let delta_mono = mono_ms - prev.monotonic_clock_ms;
        let delta_boot = boot_ms - prev.boot_clock_ms;
        let suspend_ms = (delta_boot - delta_mono).max(0);

        if delta_mono > PER_GAP_THRESHOLD_MS {
            self.state.unexpected_gap.record(utc_ms, delta_mono);
            if self.state.unexpected_gap.crossed_budget(utc_ms) {
                let _ = emitter.send(Upload {
                    risk: HIGH_RISK_LIFECYCLE_ALERT,
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::UnexpectedGap,
                    },
                });
            }
        }

        if suspend_ms >= SUSPEND_MIN_MS {
            let _ = emitter.send(Upload {
                risk: 0.0,
                kind: UploadKind::Lifecycle {
                    kind: LifecycleKind::SuspendDetected {
                        duration_ms: suspend_ms,
                    },
                },
            });
        }

        Ok(())
    }

    fn poll_login_logout(
        &mut self,
        utc_ms: i64,
        boot_ms: i64,
        mono_ms: i64,
        emitter: &Emitter,
    ) -> CoreResult<()> {
        self.state.last_login_poll_at_ms = utc_ms;

        if let Some(login_ms) = self.hooks.get_last_login_utc_ms()? {
            self.observe_login(login_ms, utc_ms, boot_ms, mono_ms, emitter)?;
        }
        if let Some(logout_ms) = self.hooks.get_last_logout_utc_ms()? {
            self.observe_logout(logout_ms, emitter)?;
        }

        Ok(())
    }

    fn observe_login(
        &mut self,
        login_ms: i64,
        utc_ms: i64,
        boot_ms: i64,
        mono_ms: i64,
        emitter: &Emitter,
    ) -> CoreResult<()> {
        if login_ms <= self.state.last_login_utc_ms {
            return Ok(());
        }
        self.state.login_id += 1;
        self.state.last_login_utc_ms = login_ms;
        let _ = emitter.send(Upload {
            risk: 0.0,
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::SystemLogin { utc_ms: login_ms },
            },
        });
        self.evaluate_unexpected_start(login_ms, utc_ms, boot_ms, mono_ms, emitter)
    }

    fn observe_logout(&mut self, logout_ms: i64, emitter: &Emitter) -> CoreResult<()> {
        if logout_ms <= self.state.last_logout_utc_ms {
            return Ok(());
        }
        self.state.last_logout_utc_ms = logout_ms;
        let _ = emitter.send(Upload {
            risk: 0.0,
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::SystemLogout { utc_ms: logout_ms },
            },
        });
        self.evaluate_unexpected_stop(logout_ms, emitter)
    }

    /// Unexpected start: awake time between a known login and the first
    /// heartbeat sample observed since, exceeding the per-gap threshold —
    /// the session was live and awake but we weren't running yet (disabled
    /// autostart, late launch). Suspend accumulated since boot is backed out
    /// conservatively (see architecture notes): we have no clock sample at
    /// the exact moment of login, only at first-observed-heartbeat, so this
    /// slightly over-excuses suspend that happened before login — which only
    /// ever shrinks the alert window, never invents one.
    fn evaluate_unexpected_start(
        &mut self,
        login_ms: i64,
        utc_ms: i64,
        boot_ms: i64,
        mono_ms: i64,
        emitter: &Emitter,
    ) -> CoreResult<()> {
        let raw_gap = utc_ms - login_ms;
        if raw_gap <= 0 {
            return Ok(());
        }
        let suspend_since_boot = (boot_ms - mono_ms).max(0);
        let excusable = suspend_since_boot.min(raw_gap);
        let gap = raw_gap - excusable;

        if gap > PER_GAP_THRESHOLD_MS {
            self.state.unexpected_start.record(utc_ms, gap);
            if self.state.unexpected_start.crossed_budget(utc_ms) {
                let _ = emitter.send(Upload {
                    risk: HIGH_RISK_LIFECYCLE_ALERT,
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::UnexpectedStart,
                    },
                });
            }
        }

        Ok(())
    }

    /// Unexpected stop: gap between the last known-alive sample and the
    /// session's logout, exceeding the per-gap threshold — we stopped
    /// running before the session ended (deliberate quit or kill before
    /// logout). When the logout timestamp is itself a reconstructed floor
    /// (unclean shutdown), it sits at or before the true end, so the gap can
    /// only shrink, never be invented — a simultaneous force-kill + power
    /// pull correctly produces ~0 gap and stays silent.
    fn evaluate_unexpected_stop(&mut self, logout_ms: i64, emitter: &Emitter) -> CoreResult<()> {
        let Some(last_sample) = self.state.last_sample else {
            return Ok(());
        };
        let gap = (logout_ms - last_sample.utc_ms).max(0);

        if gap > PER_GAP_THRESHOLD_MS {
            self.state.unexpected_stop.record(logout_ms, gap);
            if self.state.unexpected_stop.crossed_budget(logout_ms) {
                let _ = emitter.send(Upload {
                    risk: HIGH_RISK_LIFECYCLE_ALERT,
                    kind: UploadKind::LifecycleAlert {
                        reason: AlertReason::UnexpectedStop,
                    },
                });
            }
        }

        Ok(())
    }
}

impl Observer for LifecycleModule {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "lifecycle"
    }

    fn init(&mut self, _bus: &mut EventBus, state: StateType) -> CoreResult<()> {
        if !state.is_null() {
            self.state = serde_json::from_value(state)?;
        }
        Ok(())
    }

    fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
        crate::dispatch_event!(event, {
            _: Ping => self.handle_ping(emitter),
            _: ProcessStarted => {
                // Force a fresh login/logout poll on the very next Ping,
                // rather than waiting out the coarse throttle — a process
                // restart is exactly when a login/logout is likely to have
                // changed underneath us.
                self.state.last_login_poll_at_ms = 0;
                Ok(())
            },
            ev: ProcessStopped => {
                if matches!(ev.0, ProcessStoppedReason::User) {
                    let _ = emitter.send(Upload {
                        risk: EXTRA_HIGH_RISK,
                        kind: UploadKind::LifecycleAlert { reason: AlertReason::UserStop },
                    });
                }
                Ok(())
            },
            _: StatusRequest => {
                let last_loop_at_ms = self.state.last_sample.map(|s| s.utc_ms);
                let _ = emitter.send(PartialStatus::Lifecycle { is_running: true, last_loop_at_ms });
                Ok(())
            },
            _: UserStopRequested => Ok(()),
        })
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.state)?)
    }
}

/// Stand-in for platforms where `PlatformConfig::lifecycle_enabled` is
/// `false` (currently only iOS, which has no boot/shutdown/session signal
/// available to its Safari-extension-host process). Keeps `name() ==
/// "lifecycle"` so the state-file key stays stable and answers
/// `StatusRequest` so `StatusModule`'s partial-status count is still
/// satisfied, but otherwise does nothing.
#[derive(Default)]
pub struct NoopLifecycleModule;

impl NoopLifecycleModule {
    pub fn new() -> Self {
        Self
    }
}

impl Observer for NoopLifecycleModule {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "lifecycle"
    }

    fn init(&mut self, _bus: &mut EventBus, _state: StateType) -> CoreResult<()> {
        Ok(())
    }

    fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
        crate::dispatch_event!(event, {
            _: StatusRequest => {
                let _ = emitter.send(PartialStatus::Lifecycle { is_running: true, last_loop_at_ms: None });
                Ok(())
            },
        })
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(StateType::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EXTRA_HIGH_RISK, HIGH_RISK_LIFECYCLE_ALERT, LifecycleModule, ProcessStarted, ProcessStopped,
    };
    use crate::events::Ping;
    use crate::model::PartialStatus;
    use crate::model::{AlertReason, LifecycleKind, ProcessStoppedReason, UploadKind};
    use crate::module::status::StatusRequest;
    use crate::module::upload::Upload;
    use crate::testing::EventTester;

    #[test]
    fn status_request_emits_lifecycle_partial_status() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(1, StatusRequest);
        t.assert_like::<PartialStatus>(crate::like!(PartialStatus::Lifecycle { .. }));
    }

    #[test]
    fn routine_process_start_stop_produces_no_log_row() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.emit(1, ProcessStarted);
        t.emit(2, ProcessStopped(ProcessStoppedReason::Other));
        t.emit(3, ProcessStopped(ProcessStoppedReason::Shutdown));
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::Lifecycle { .. },
            ..
        }));
    }

    #[test]
    fn login_poll_emits_system_login_and_tracks_state() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.platform.set_last_login(Some(500));
        t.emit(1, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::SystemLogin { utc_ms: 500 }
            },
            ..
        }));
        assert_eq!(t.observer::<LifecycleModule>().state.last_login_utc_ms, 500);
        assert_eq!(t.observer::<LifecycleModule>().state.login_id, 1);
    }

    #[test]
    fn logout_poll_emits_system_logout() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.platform.set_last_logout(Some(500));
        t.emit(1, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::SystemLogout { utc_ms: 500 }
            },
            ..
        }));
        assert_eq!(
            t.observer::<LifecycleModule>().state.last_logout_utc_ms,
            500
        );
    }

    #[test]
    fn sub_budget_unexpected_gap_does_not_alert() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        // Seed a prior sample; boot/monotonic default to tracking the mock
        // wall clock (no suspend) unless overridden.
        t.platform.set_boot_clock_ms(1_000);
        t.platform.set_monotonic_clock_ms(1_000);
        t.emit(1, Ping);
        t.clear_captured();

        // A single 30s gap: over the 10s per-gap threshold (recorded) but
        // under the 60s sliding-window budget, so no alert fires.
        t.platform.set_boot_clock_ms(31_000);
        t.platform.set_monotonic_clock_ms(31_000);
        t.emit(31, Ping);
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UnexpectedGap
            },
            ..
        }));
    }

    #[test]
    fn accumulated_unexpected_gaps_cross_budget_and_alert() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.platform.set_boot_clock_ms(1_000);
        t.platform.set_monotonic_clock_ms(1_000);
        t.emit(1, Ping);
        t.clear_captured();

        // Three 25s gaps: 25 -> 50 (both under the 60s budget), then 75 >= 60 alerts.
        t.platform.set_boot_clock_ms(26_000);
        t.platform.set_monotonic_clock_ms(26_000);
        t.emit(26, Ping);
        t.platform.set_boot_clock_ms(51_000);
        t.platform.set_monotonic_clock_ms(51_000);
        t.emit(51, Ping);
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UnexpectedGap
            },
            ..
        }));
        t.clear_captured();

        t.platform.set_boot_clock_ms(76_000);
        t.platform.set_monotonic_clock_ms(76_000);
        t.emit(76, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UnexpectedGap
            },
            ..
        }));
        let alert = t
            .captured::<Upload>()
            .into_iter()
            .find(|e| {
                matches!(
                    e.kind,
                    UploadKind::LifecycleAlert {
                        reason: AlertReason::UnexpectedGap
                    }
                )
            })
            .unwrap();
        assert!(
            alert.risk >= HIGH_RISK_LIFECYCLE_ALERT && alert.risk < EXTRA_HIGH_RISK,
            "unexpected-gap alert should be high but not immediate, got {}",
            alert.risk
        );
    }

    #[test]
    fn cooldown_suppresses_repeat_unexpected_gap_alert() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.platform.set_boot_clock_ms(1_000);
        t.platform.set_monotonic_clock_ms(1_000);
        t.emit(1, Ping);

        // First gap of 70s crosses budget and alerts.
        t.platform.set_boot_clock_ms(71_000);
        t.platform.set_monotonic_clock_ms(71_000);
        t.emit(71, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UnexpectedGap
            },
            ..
        }));
        t.clear_captured();

        // 40s later (< 5-min cooldown): another 70s gap crosses budget again but is suppressed.
        t.platform.set_boot_clock_ms(181_000);
        t.platform.set_monotonic_clock_ms(181_000);
        t.emit(181, Ping);
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UnexpectedGap
            },
            ..
        }));
        t.clear_captured();

        // Well past the cooldown: the chronic stall re-alerts.
        t.platform.set_boot_clock_ms(600_000);
        t.platform.set_monotonic_clock_ms(600_000);
        t.emit(600, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UnexpectedGap
            },
            ..
        }));
    }

    #[test]
    fn suspend_excuses_unexpected_gap() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.platform.set_boot_clock_ms(1_000);
        t.platform.set_monotonic_clock_ms(1_000);
        t.emit(1, Ping);
        t.clear_captured();

        // 10 minutes of wall-clock/boot time pass, but monotonic only
        // advances 30s (the machine was suspended for the rest) — no
        // UnexpectedGap alert, but a SuspendDetected log is emitted.
        t.platform.set_boot_clock_ms(601_000);
        t.platform.set_monotonic_clock_ms(31_000);
        t.emit(601, Ping);
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UnexpectedGap
            },
            ..
        }));
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Lifecycle {
                kind: LifecycleKind::SuspendDetected { .. }
            },
            ..
        }));
    }

    #[test]
    fn unexpected_start_alerts_when_login_precedes_first_sample_by_more_than_threshold() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.platform.set_last_login(Some(100));
        t.platform.set_boot_clock_ms(61_000);
        t.platform.set_monotonic_clock_ms(61_000);
        t.emit(61, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UnexpectedStart
            },
            ..
        }));
    }

    #[test]
    fn unexpected_start_excused_by_suspend_since_boot() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.platform.set_last_login(Some(100));
        // 60s of wall-clock/boot time since login, but monotonic shows only
        // 5s of that was actually awake (55s of suspend) — the awake gap
        // (5s) is under the per-gap threshold, so no alert.
        t.platform.set_boot_clock_ms(60_000);
        t.platform.set_monotonic_clock_ms(5_000);
        t.emit(60, Ping);
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UnexpectedStart
            },
            ..
        }));
    }

    #[test]
    fn unexpected_start_silent_with_no_known_login() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        // No `set_last_login` call — hook returns None, so there's nothing to
        // anchor an unexpected-start check against.
        t.emit(60, Ping);
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UnexpectedStart
            },
            ..
        }));
    }

    #[test]
    fn unexpected_stop_alerts_when_logout_arrives_well_after_last_sample() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.platform.set_boot_clock_ms(1_000);
        t.platform.set_monotonic_clock_ms(1_000);
        t.emit(1, Ping);
        t.clear_captured();

        // Logout is reported 60s after our last heartbeat. Force an immediate
        // poll (rather than waiting out the 5-min throttle) via ProcessStarted.
        t.platform.set_last_logout(Some(61_000));
        t.emit(61, ProcessStarted);
        t.emit(61, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UnexpectedStop
            },
            ..
        }));
        let alert = t
            .captured::<Upload>()
            .into_iter()
            .find(|e| {
                matches!(
                    e.kind,
                    UploadKind::LifecycleAlert {
                        reason: AlertReason::UnexpectedStop
                    }
                )
            })
            .unwrap();
        assert!(
            alert.risk >= HIGH_RISK_LIFECYCLE_ALERT && alert.risk < EXTRA_HIGH_RISK,
            "unexpected-stop alert should be high but not immediate, got {}",
            alert.risk
        );
    }

    #[test]
    fn floor_reconstruction_landing_on_last_sample_does_not_alert() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.platform.set_boot_clock_ms(60_000);
        t.platform.set_monotonic_clock_ms(60_000);
        t.emit(60, Ping);
        t.clear_captured();

        // A reconstructed logout floor that lands at/before our last known
        // sample (the "simultaneous force-kill + power pull" case) produces
        // zero/near-zero gap and must not alert.
        t.platform.set_last_logout(Some(60_000));
        t.emit(120, ProcessStarted);
        t.emit(120, Ping);
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UnexpectedStop
            },
            ..
        }));
    }

    #[test]
    fn reboot_regression_does_not_corrupt_mid_session_math() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.platform.set_boot_clock_ms(500_000);
        t.platform.set_monotonic_clock_ms(500_000);
        t.emit(500, Ping);
        t.clear_captured();

        // Reboot: boot/monotonic clocks reset to small values while wall
        // clock keeps climbing. Must not be misread as a huge mid-session gap.
        t.platform.set_boot_clock_ms(2_000);
        t.platform.set_monotonic_clock_ms(2_000);
        t.emit(600, Ping);
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UnexpectedGap
            },
            ..
        }));
        assert_eq!(
            t.observer::<LifecycleModule>().state.last_boot_clock_ms,
            2_000
        );
    }

    #[test]
    fn user_stop_fires_immediate_alert_and_later_gap_still_evaluated() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();
        t.platform.set_boot_clock_ms(1_000);
        t.platform.set_monotonic_clock_ms(1_000);
        t.emit(1, Ping);

        // Explicit user-initiated stop: immediate high-risk alert.
        t.emit(2, ProcessStopped(ProcessStoppedReason::User));
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UserStop
            },
            ..
        }));
        let user_stop = t
            .captured::<Upload>()
            .into_iter()
            .find(|e| {
                matches!(
                    e.kind,
                    UploadKind::LifecycleAlert {
                        reason: AlertReason::UserStop
                    }
                )
            })
            .unwrap();
        assert!(
            user_stop.risk >= EXTRA_HIGH_RISK,
            "user stop should be immediate/extra-high risk"
        );
        t.clear_captured();

        // The session's logout eventually arrives (via the poll, forced
        // immediately rather than waiting out the throttle), long after the
        // stop — the resulting gap is still independently evaluated and
        // alerts, even though the stop was user-initiated.
        t.platform.set_last_logout(Some(63_000));
        t.emit(63, ProcessStarted);
        t.emit(63, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::LifecycleAlert {
                reason: AlertReason::UnexpectedStop
            },
            ..
        }));
    }

    #[test]
    fn state_round_trips_through_save_and_load() {
        let mut b = EventTester::builder();
        b.add(LifecycleModule::new(Box::new(b.platform())));
        let mut t = b.build();

        t.platform.set_last_login(Some(30_000));
        t.emit(30, Ping);
        t.platform.set_last_login(None);

        {
            let s = &t.observer::<LifecycleModule>().state;
            assert_eq!(s.last_login_utc_ms, 30_000);
            assert_eq!(s.login_id, 1);
        }

        let saved = t.bus.save().unwrap();

        let mut b2 = EventTester::builder();
        b2.add(LifecycleModule::new(Box::new(b2.platform())));
        b2.with_state(saved);
        let mut t2 = b2.build();

        let s2 = &t2.observer::<LifecycleModule>().state;
        assert_eq!(s2.last_login_utc_ms, 30_000);
        assert_eq!(s2.login_id, 1);
        assert!(s2.last_sample.is_some());
    }
}
