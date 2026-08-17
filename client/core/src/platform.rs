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

/// Live, per-tick I/O feeding the lifecycle late-wakeup model. See
/// `daemon::lifecycle` for how these hooks are used.
pub trait LifecycleHooks: Send + Sync + 'static {
    /// Same semantics/default as `ScreenshotHooks::get_time_utc_ms`.
    fn get_utc_clock_ms(&self) -> CoreResult<i64> {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| CoreError::CommandFailed(e.to_string()))?;
        i64::try_from(duration.as_millis())
            .map_err(|_| CoreError::InvalidState("system clock overflow"))
    }

    /// Start of the current expected-running window (OS session/user login),
    /// as a UTC timestamp. `None` if not yet knowable.
    fn get_last_login_utc_ms(&self) -> CoreResult<Option<i64>>;

    /// End of the most recently closed expected-running window (OS
    /// session/user logout), as a UTC timestamp. `None` while the current
    /// session is presumed still open.
    fn get_last_logout_utc_ms(&self) -> CoreResult<Option<i64>>;
}

/// Marker trait for platform implementations. Kept as a supertrait rather
/// than requiring call sites to name both `ScreenshotHooks` and
/// `LifecycleHooks` individually.
pub trait PlatformHooks: ScreenshotHooks + LifecycleHooks {}

impl<T: ScreenshotHooks + LifecycleHooks> PlatformHooks for T {}
