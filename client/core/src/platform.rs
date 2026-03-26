use crate::error::CoreResult;
use crate::model::Screenshot;

pub trait PlatformHooks {
    fn take_screenshot(&self) -> CoreResult<Screenshot>;
    fn get_time_utc_ms(&self) -> CoreResult<i64>;
}
