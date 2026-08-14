use std::sync::Arc;

use crate::api::{ApiTransport, ReqwestApiClient};
use crate::config::Config;
use crate::error::CoreResult;
use crate::events::bus::Observer;
use crate::module::auth::AuthModule;
use crate::module::capture_availability::CaptureAvailabilityModule;
use crate::module::heartbeat::HeartbeatModule;
use crate::module::lifecycle::{LifecycleModule, NoopLifecycleModule};
use crate::module::screenshot::ScreenshotModule;
use crate::module::status::StatusModule;
use crate::module::upload::UploadModule;
use crate::platform::{LifecycleHooks, PlatformConfig, ScreenshotHooks};

/// Number of observers that emit a `PartialStatus` in reply to a `StatusRequest`:
/// `AuthModule`, `LifecycleModule` (or `NoopLifecycleModule`), and `UploadModule`.
const STATUS_PARTIAL_COUNT: usize = 3;

/// Build the default set of 7 observer modules from the given config, platform,
/// and API transport. The returned modules are ready to be passed to
/// [`EventBus::new`].
///
/// `platform_config.lifecycle_enabled` decides whether a real `LifecycleModule`
/// or an inert `NoopLifecycleModule` is constructed — `false` only on iOS,
/// which has no boot/shutdown/session signal available to it. Most platforms
/// can pass `PlatformConfig::default()`.
pub fn build_default_modules<P, A>(
    config: Config,
    platform: P,
    api: A,
    platform_config: PlatformConfig,
) -> CoreResult<Vec<Box<dyn Observer>>>
where
    P: ScreenshotHooks + LifecycleHooks + Clone,
    A: ApiTransport + Clone + Send + Sync + 'static,
{
    let screenshot_interval_ms = config.screenshot_interval.as_millis() as i64;
    let batch_interval_ms = config.batch_interval.as_millis() as i64;
    let device_name = config.device_name.clone();
    let platform_name = config.platform_name.clone();
    let state_dir = config.state_dir.clone();

    let lifecycle: Box<dyn Observer> = if platform_config.lifecycle_enabled {
        Box::new(LifecycleModule::new(Box::new(platform.clone())))
    } else {
        Box::new(NoopLifecycleModule::new())
    };

    let observers: Vec<Box<dyn Observer>> = vec![
        lifecycle,
        Box::new(ScreenshotModule::new(
            Arc::new(platform.clone()),
            screenshot_interval_ms,
        )),
        Box::new(
            UploadModule::new(Box::new(platform.clone()), api.clone(), batch_interval_ms)
                .with_error_log(crate::storage::FileStateStore::new(&state_dir)?),
        ),
        Box::new(CaptureAvailabilityModule::new(Box::new(platform.clone()))),
        Box::new(HeartbeatModule::new(Box::new(platform))),
        Box::new(AuthModule::new(api, device_name, platform_name)),
        Box::new(StatusModule::new(STATUS_PARTIAL_COUNT)),
    ];

    Ok(observers)
}

/// Convenience wrapper that defaults to [`ReqwestApiClient`]. See
/// `build_default_modules` for `platform_config`.
pub fn build_default_modules_reqwest<P>(
    config: Config,
    platform: P,
    platform_config: PlatformConfig,
) -> CoreResult<Vec<Box<dyn Observer>>>
where
    P: ScreenshotHooks + LifecycleHooks + Clone,
{
    let api = ReqwestApiClient::new(&config)?;
    build_default_modules(config, platform, api, platform_config)
}
