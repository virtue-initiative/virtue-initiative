pub const DEFAULT_BASE_API_URL: &str = "https://bepure-api.anb.codes";
pub const BASE_API_URL_ENV_VAR: &str = "BEPURE_BASE_API_URL";
pub const CAPTURE_INTERVAL_SECONDS_ENV_VAR: &str = "BEPURE_CAPTURE_INTERVAL_SECONDS";
pub const DEFAULT_CAPTURE_INTERVAL_SECONDS: u64 = 300;
pub const MIN_CAPTURE_INTERVAL_SECONDS: u64 = 15;

pub fn resolve_base_api_url() -> String {
    std::env::var(BASE_API_URL_ENV_VAR)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_API_URL.to_string())
}

pub fn clamp_capture_interval_seconds(seconds: u64) -> u64 {
    seconds.max(MIN_CAPTURE_INTERVAL_SECONDS)
}

pub fn resolve_capture_interval_seconds(default_seconds: u64) -> u64 {
    std::env::var(CAPTURE_INTERVAL_SECONDS_ENV_VAR)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(clamp_capture_interval_seconds)
        .unwrap_or_else(|| clamp_capture_interval_seconds(default_seconds))
}

pub mod auth;
pub mod error;
pub mod image_pipeline;
pub mod models;
pub mod queue;
pub mod schedule;
pub mod token_store;
pub mod upload;

pub use auth::{AuthClient, AuthClientConfig};
pub use error::{CoreError, CoreResult};
pub use image_pipeline::{ImageOutputFormat, ImagePipeline, ImagePipelineConfig, ProcessedImage};
pub use queue::{BufferedUpload, PersistentQueue, QueueEnqueueResult};
pub use schedule::{CaptureSchedulePolicy, CaptureScheduleState, RetryPolicy};
pub use token_store::{FileTokenStore, MemoryTokenStore, TokenStore};
pub use upload::{QueueProcessReport, UploadClient, UploadClientConfig};
