pub const DEFAULT_BASE_API_URL: &str = "https://bepure-api.anb.codes";
pub const BASE_API_URL_ENV_VAR: &str = "BEPURE_BASE_API_URL";

pub fn resolve_base_api_url() -> String {
    std::env::var(BASE_API_URL_ENV_VAR)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_API_URL.to_string())
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
