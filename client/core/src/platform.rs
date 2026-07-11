use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{CoreError, CoreResult};
use crate::model::Screenshot;

/// Object-safe subset of platform capabilities. Used as a trait object
/// (`Box<dyn ScreenshotHooks>`) in modules.
pub trait ScreenshotHooks: Send + Sync + 'static {
    fn take_screenshot(&self) -> CoreResult<Screenshot>;

    fn get_time_utc_ms(&self) -> CoreResult<i64> {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| CoreError::CommandFailed(e.to_string()))?;
        i64::try_from(duration.as_millis())
            .map_err(|_| CoreError::InvalidState("system clock overflow"))
    }

    fn get_last_shutdown_time_utc_ms(&self) -> CoreResult<Option<i64>>;
    fn get_last_startup_time_utc_ms(&self) -> CoreResult<Option<i64>>;

    /// True if the session is locked or a screensaver is active.
    ///
    /// While locked/screensaving the user cannot be viewing real content, so the
    /// screenshot module suppresses capture unconditionally. `Ok(false)` is the
    /// fail-safe default when the state can't be determined (unknown session) —
    /// i.e. fall back to the diff gate, never silently suppress. The default
    /// implementation returns `Ok(false)` for platforms without the concept
    /// (e.g. mobile); desktop platforms override it.
    fn is_locked_or_screensaver(&self) -> CoreResult<bool> {
        Ok(false)
    }
}

/// Marker trait for platform implementations. Kept as a supertrait of
/// `ScreenshotHooks` to minimize churn at call sites while `Custom` event
/// support is removed.
pub trait PlatformHooks: ScreenshotHooks {}

/// Static, per-platform capabilities that shape module behavior at startup —
/// as opposed to `ScreenshotHooks`, which is live platform I/O queried on every
/// tick. Passed once when assembling the observer modules (see
/// `assembly::build_default_modules`) rather than modeled as trait methods,
/// since these never change at runtime and aren't tied to a specific platform
/// I/O call.
#[derive(Debug, Clone, Copy)]
pub struct PlatformConfig {
    /// Whether the platform can reliably tell the lifecycle module about
    /// suspend/resume (sleep/wake) — i.e. whether a ping stall can be
    /// attributed to a legitimate, OS-driven pause rather than an unexplained
    /// one.
    ///
    /// Desktop platforms emit explicit `ComputerSuspended`/`ComputerResumed`
    /// events off real OS power notifications, so a suspend period is
    /// bracketed and never counted as a stall in the first place — `true` is
    /// correct for them. Some platforms have no way to observe this at all: on
    /// iOS the monitoring process is a short-lived Safari extension host that
    /// the OS can suspend the instant the device locks, with no notification
    /// delivered to that process (extensions have no `UIApplication`) and no
    /// way to reconstruct after the fact whether a given stall was a lock, a
    /// suspicious pause, or something else — every stall looks identical. When
    /// `false`, `LifecycleModule` must not evaluate `PingGapWhileRunning` at
    /// all, since we have no way to distinguish a benign gap from a suspicious
    /// one.
    pub supports_sleep_wake_detection: bool,
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self {
            supports_sleep_wake_detection: true,
        }
    }
}
