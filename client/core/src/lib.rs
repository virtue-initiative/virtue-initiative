pub const DEFAULT_BASE_API_URL: &str = "https://api.bepure.anb.codes";
pub const BASE_API_URL_ENV_VAR: &str = "VIRTUE_BASE_API_URL";
pub const CAPTURE_INTERVAL_SECONDS_ENV_VAR: &str = "VIRTUE_CAPTURE_INTERVAL_SECONDS";
pub const BATCH_WINDOW_SECONDS_ENV_VAR: &str = "VIRTUE_BATCH_WINDOW_SECONDS";
pub const DEFAULT_CAPTURE_INTERVAL_SECONDS: u64 = 300;
pub const DEFAULT_BATCH_WINDOW_SECONDS: u64 = 3600;
pub const MIN_CAPTURE_INTERVAL_SECONDS: u64 = 15;
pub const MIN_BATCH_WINDOW_SECONDS: u64 = 1;

pub fn resolve_base_api_url() -> String {
    std::env::var(BASE_API_URL_ENV_VAR)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_API_URL.to_string())
}

/// Applies variables embedded from `.env.dev` at compile time.
/// Only has an effect in debug builds; runtime env vars always take precedence.
pub fn apply_dev_env() {
    #[cfg(debug_assertions)]
    {
        if let Some(val) = option_env!("VIRTUE_BASE_API_URL") {
            if std::env::var("VIRTUE_BASE_API_URL").is_err() {
                unsafe { std::env::set_var("VIRTUE_BASE_API_URL", val) };
            }
        }
        if let Some(val) = option_env!("VIRTUE_CAPTURE_INTERVAL_SECONDS") {
            if std::env::var("VIRTUE_CAPTURE_INTERVAL_SECONDS").is_err() {
                unsafe { std::env::set_var("VIRTUE_CAPTURE_INTERVAL_SECONDS", val) };
            }
        }
        if let Some(val) = option_env!("VIRTUE_BATCH_WINDOW_SECONDS") {
            if std::env::var("VIRTUE_BATCH_WINDOW_SECONDS").is_err() {
                unsafe { std::env::set_var("VIRTUE_BATCH_WINDOW_SECONDS", val) };
            }
        }
    }
}

pub fn clamp_capture_interval_seconds(seconds: u64) -> u64 {
    seconds.max(MIN_CAPTURE_INTERVAL_SECONDS)
}

pub fn resolve_capture_interval_seconds() -> u64 {
    std::env::var(CAPTURE_INTERVAL_SECONDS_ENV_VAR)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(clamp_capture_interval_seconds)
        .unwrap_or_else(|| clamp_capture_interval_seconds(DEFAULT_CAPTURE_INTERVAL_SECONDS))
}

pub fn clamp_batch_window_seconds(seconds: u64) -> u64 {
    seconds.max(MIN_BATCH_WINDOW_SECONDS)
}

pub fn resolve_batch_window_seconds() -> u64 {
    std::env::var(BATCH_WINDOW_SECONDS_ENV_VAR)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(clamp_batch_window_seconds)
        .unwrap_or_else(|| clamp_batch_window_seconds(DEFAULT_BATCH_WINDOW_SECONDS))
}

pub mod auth;
pub mod batch;
pub mod crypto;
pub mod error;
pub mod hash_chain;
pub mod image_pipeline;
pub mod models;
pub mod queue;
pub mod schedule;
pub mod token_store;
pub mod upload;

pub use auth::{AuthClient, AuthClientConfig};
pub use batch::{BatchBlob, BatchItem};
pub use crypto::{decrypt, derive_key, encrypt};
pub use error::{CoreError, CoreResult};
pub use hash_chain::ChainHasher;
pub use image_pipeline::{ImagePipeline, ProcessedImage};
pub use schedule::{CaptureSchedulePolicy, CaptureScheduleState, RetryPolicy};
pub use token_store::{FileTokenStore, MemoryTokenStore, TokenStore};
pub use upload::{UploadClient, UploadClientConfig, sha256_bytes, sha256_hex};
