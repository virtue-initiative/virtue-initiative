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

/// Live, per-tick I/O feeding the lifecycle gap-detection model. See
/// `module::lifecycle` for how these five hooks are used.
pub trait LifecycleHooks: Send + Sync + 'static {
    /// Same semantics/default as `ScreenshotHooks::get_time_utc_ms`.
    fn get_utc_clock_ms(&self) -> CoreResult<i64> {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| CoreError::CommandFailed(e.to_string()))?;
        i64::try_from(duration.as_millis())
            .map_err(|_| CoreError::InvalidState("system clock overflow"))
    }

    /// Time since boot, in milliseconds. **Includes** time spent suspended.
    /// `CLOCK_BOOTTIME` (Linux) / `elapsedRealtime()` (Android) /
    /// `mach_continuous_time()` (macOS) / `QueryInterruptTime` (Windows).
    fn get_boot_clock_ms(&self) -> CoreResult<i64>;

    /// Time since boot, in milliseconds. **Excludes** time spent suspended
    /// (pauses while asleep). `CLOCK_MONOTONIC` (Linux) / `uptimeMillis()`
    /// (Android) / `mach_absolute_time()` (macOS) /
    /// `QueryUnbiasedInterruptTime` (Windows).
    ///
    /// `get_boot_clock_ms() - get_monotonic_clock_ms()` is the total suspend
    /// time accumulated since boot.
    fn get_monotonic_clock_ms(&self) -> CoreResult<i64>;

    /// Start of the current expected-running window (OS session/user login),
    /// as a UTC timestamp. `None` if not yet knowable.
    fn get_last_login_utc_ms(&self) -> CoreResult<Option<i64>>;

    /// End of the most recently closed expected-running window (OS
    /// session/user logout), as a UTC timestamp. May be an approximate floor
    /// reconstructed from OS shutdown records rather than an exact clean
    /// logout — see `module::lifecycle`. `None` while the current session is
    /// presumed still open.
    fn get_last_logout_utc_ms(&self) -> CoreResult<Option<i64>>;
}

/// Marker trait for platform implementations. Kept as a supertrait rather
/// than requiring call sites to name both `ScreenshotHooks` and
/// `LifecycleHooks` individually.
pub trait PlatformHooks: ScreenshotHooks + LifecycleHooks {}

/// Static, per-platform capabilities that shape module behavior at startup —
/// as opposed to `ScreenshotHooks`/`LifecycleHooks`, which are live platform
/// I/O queried on every tick. Passed once when assembling the observer
/// modules (see `assembly::build_default_modules`) rather than modeled as
/// trait methods, since these never change at runtime and aren't tied to a
/// specific platform I/O call.
#[derive(Debug, Clone, Copy)]
pub struct PlatformConfig {
    /// Whether this platform has a working lifecycle model at all — i.e.
    /// whether `LifecycleHooks` can be trusted to report meaningful boot/
    /// monotonic clocks and login/logout timestamps.
    ///
    /// `false` only on iOS: the monitoring process is a short-lived Safari
    /// extension host that the OS can suspend the instant the device locks,
    /// with no notification delivered to that process (extensions have no
    /// `UIApplication`) and no boot/shutdown/session API surface available to
    /// it at all — every stall looks identical to every other, so there is no
    /// way to build a meaningful expected-running-window model. When `false`,
    /// `LifecycleModule` is not constructed at all; a no-op stands in.
    pub lifecycle_enabled: bool,
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self {
            lifecycle_enabled: true,
        }
    }
}
