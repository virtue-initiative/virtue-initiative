use crate::error::CoreResult;
use crate::model::Screenshot;

/// Object-safe subset of platform capabilities. Used as a trait object
/// (`Box<dyn ScreenshotHooks>`) in modules.
pub trait ScreenshotHooks: Send + Sync + 'static {
    fn take_screenshot(&self) -> CoreResult<Screenshot>;
    fn get_time_utc_ms(&self) -> CoreResult<i64>;
    fn get_last_shutdown_time_utc_ms(&self) -> CoreResult<Option<i64>>;
    fn get_last_startup_time_utc_ms(&self) -> CoreResult<Option<i64>>;
}

/// Marker trait for platform implementations. Kept as a supertrait of
/// `ScreenshotHooks` to minimize churn at call sites while `Custom` event
/// support is removed.
pub trait PlatformHooks: ScreenshotHooks {}
