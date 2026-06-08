use crate::error::CoreResult;
use crate::model::Screenshot;

/// Object-safe subset of platform capabilities. Used as a trait object (`Box<dyn ScreenshotHooks>`)
/// in observers. Every `PlatformHooks` implementation must also implement this trait directly.
pub trait ScreenshotHooks: Send + Sync + 'static {
    fn take_screenshot(&self) -> CoreResult<Screenshot>;
    fn get_time_utc_ms(&self) -> CoreResult<i64>;
    fn get_last_shutdown_time_utc_ms(&self) -> CoreResult<Option<i64>>;
    fn get_last_startup_time_utc_ms(&self) -> CoreResult<Option<i64>>;
}

pub trait PlatformHooks: ScreenshotHooks {
    type CustomEvent: Send + Clone + std::fmt::Debug + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static;
}
