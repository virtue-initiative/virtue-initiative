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
