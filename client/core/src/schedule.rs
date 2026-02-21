use std::time::Duration;

use rand::Rng;

#[derive(Clone, Copy, Debug, Default)]
pub struct CaptureScheduleState {
    pub consecutive_failures: u32,
}

#[derive(Clone, Debug)]
pub struct CaptureSchedulePolicy {
    pub base_interval: Duration,
    pub jitter_ratio: f64,
    pub backoff_multiplier: f64,
    pub max_backoff: Duration,
}

impl Default for CaptureSchedulePolicy {
    fn default() -> Self {
        Self {
            base_interval: Duration::from_secs(300),
            jitter_ratio: 0.15,
            backoff_multiplier: 2.0,
            max_backoff: Duration::from_secs(3600),
        }
    }
}

impl CaptureSchedulePolicy {
    pub fn next_delay<R: Rng + ?Sized>(
        &self,
        state: &mut CaptureScheduleState,
        last_capture_succeeded: bool,
        rng: &mut R,
    ) -> Duration {
        if last_capture_succeeded {
            state.consecutive_failures = 0;
            return apply_jitter(self.base_interval, self.jitter_ratio, rng);
        }

        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        let factor = self
            .backoff_multiplier
            .powi(state.consecutive_failures.saturating_sub(1) as i32);
        let delay = Duration::from_secs_f64(
            (self.base_interval.as_secs_f64() * factor).min(self.max_backoff.as_secs_f64()),
        );

        apply_jitter(delay, self.jitter_ratio, rng)
    }
}

#[derive(Clone, Debug)]
pub struct RetryPolicy {
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub multiplier: f64,
    pub jitter_ratio: f64,
    pub max_attempts: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(10),
            max_delay: Duration::from_secs(1800),
            multiplier: 2.0,
            jitter_ratio: 0.15,
            max_attempts: 8,
        }
    }
}

impl RetryPolicy {
    pub fn next_delay<R: Rng + ?Sized>(&self, attempt: u32, rng: &mut R) -> Duration {
        let normalized_attempt = attempt.max(1);
        let factor = self
            .multiplier
            .powi(normalized_attempt.saturating_sub(1) as i32);
        let delay = Duration::from_secs_f64(
            (self.initial_delay.as_secs_f64() * factor).min(self.max_delay.as_secs_f64()),
        );

        apply_jitter(delay, self.jitter_ratio, rng)
    }
}

fn apply_jitter<R: Rng + ?Sized>(base: Duration, jitter_ratio: f64, rng: &mut R) -> Duration {
    if jitter_ratio <= 0.0 {
        return base;
    }

    let jitter = rng.gen_range(-jitter_ratio..=jitter_ratio);
    let seconds = (base.as_secs_f64() * (1.0 + jitter)).max(1.0);
    Duration::from_secs_f64(seconds)
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::{CaptureSchedulePolicy, CaptureScheduleState, RetryPolicy};

    #[test]
    fn capture_backoff_increases_after_failures() {
        let mut rng = StdRng::seed_from_u64(42);
        let policy = CaptureSchedulePolicy {
            jitter_ratio: 0.0,
            ..CaptureSchedulePolicy::default()
        };
        let mut state = CaptureScheduleState::default();

        let first = policy.next_delay(&mut state, false, &mut rng);
        let second = policy.next_delay(&mut state, false, &mut rng);

        assert!(second > first);
    }

    #[test]
    fn capture_success_resets_backoff() {
        let mut rng = StdRng::seed_from_u64(42);
        let policy = CaptureSchedulePolicy {
            jitter_ratio: 0.0,
            ..CaptureSchedulePolicy::default()
        };
        let mut state = CaptureScheduleState::default();

        let _ = policy.next_delay(&mut state, false, &mut rng);
        let success = policy.next_delay(&mut state, true, &mut rng);

        assert_eq!(success, policy.base_interval);
        assert_eq!(state.consecutive_failures, 0);
    }

    #[test]
    fn retry_policy_caps_delay() {
        let mut rng = StdRng::seed_from_u64(7);
        let policy = RetryPolicy {
            jitter_ratio: 0.0,
            ..RetryPolicy::default()
        };

        let delay = policy.next_delay(50, &mut rng);
        assert_eq!(delay, policy.max_delay);
    }
}
