use std::collections::HashMap;
use std::path::Path;

pub const DEFAULT_BASE_API_URL: &str = "https://api.virtueinitiative.org";
pub const BASE_API_URL_ENV_VAR: &str = "VIRTUE_BASE_API_URL";
pub const CAPTURE_INTERVAL_SECONDS_ENV_VAR: &str = "VIRTUE_CAPTURE_INTERVAL_SECONDS";
pub const BATCH_WINDOW_SECONDS_ENV_VAR: &str = "VIRTUE_BATCH_WINDOW_SECONDS";
pub const DEFAULT_CAPTURE_INTERVAL_SECONDS: u64 = 300;
pub const DEFAULT_BATCH_WINDOW_SECONDS: u64 = 3600;
pub const MIN_CAPTURE_INTERVAL_SECONDS: u64 = 15;
pub const MIN_BATCH_WINDOW_SECONDS: u64 = 1;
pub const OVERRIDABLE_ENV_VARS: [&str; 3] = [
    BASE_API_URL_ENV_VAR,
    CAPTURE_INTERVAL_SECONDS_ENV_VAR,
    BATCH_WINDOW_SECONDS_ENV_VAR,
];

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
        if let Some(val) = option_env!("VIRTUE_BASE_API_URL")
            && std::env::var("VIRTUE_BASE_API_URL").is_err()
        {
            unsafe { std::env::set_var("VIRTUE_BASE_API_URL", val) };
        }
        if let Some(val) = option_env!("VIRTUE_CAPTURE_INTERVAL_SECONDS")
            && std::env::var("VIRTUE_CAPTURE_INTERVAL_SECONDS").is_err()
        {
            unsafe { std::env::set_var("VIRTUE_CAPTURE_INTERVAL_SECONDS", val) };
        }
        if let Some(val) = option_env!("VIRTUE_BATCH_WINDOW_SECONDS")
            && std::env::var("VIRTUE_BATCH_WINDOW_SECONDS").is_err()
        {
            unsafe { std::env::set_var("VIRTUE_BATCH_WINDOW_SECONDS", val) };
        }
    }
}

/// Applies default runtime env vars from an external source.
/// Existing process env vars always take precedence.
pub fn apply_env_defaults_from_map(vars: &HashMap<String, String>) {
    for key in OVERRIDABLE_ENV_VARS {
        if std::env::var(key).is_ok() {
            continue;
        }
        let Some(value) = vars.get(key) else {
            continue;
        };
        unsafe { std::env::set_var(key, value) };
    }
}

/// Loads a .env-style file (KEY=VALUE per line) and returns discovered variables.
/// Missing files return an empty map.
pub fn load_env_file(path: impl AsRef<Path>) -> HashMap<String, String> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    parse_env_file_contents(&contents)
}

fn parse_env_file_contents(contents: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();

    for raw_line in contents.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(rest) = line.strip_prefix("export ") {
            line = rest.trim_start();
        }

        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        if key.is_empty() {
            continue;
        }

        let mut value = raw_value.trim().to_string();
        if ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
            && value.len() >= 2
        {
            value = value[1..value.len() - 1].to_string();
        }

        vars.insert(key.to_string(), value);
    }

    vars
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
pub mod tray_icon;
pub mod upload;

pub use auth::{AuthClient, AuthClientConfig};
pub use batch::{BatchBlob, BatchItem};
pub use crypto::{decrypt, derive_key, encrypt};
pub use error::{CoreError, CoreResult};
pub use hash_chain::chain_step;
pub use image_pipeline::{ImagePipeline, ProcessedImage};
pub use schedule::{CaptureSchedulePolicy, CaptureScheduleState, RetryPolicy};
pub use token_store::{FileTokenStore, MemoryTokenStore, TokenStore};
pub use tray_icon::build_default_tray_icon_rgba;
pub use upload::{UploadClient, UploadClientConfig, sha256_bytes, sha256_hex, uuid_str_to_bytes};

#[cfg(test)]
mod tests {
    use super::parse_env_file_contents;

    #[test]
    fn parses_env_file_lines() {
        let parsed = parse_env_file_contents(
            r#"
            # comment
            VIRTUE_BASE_API_URL=http://localhost:8787/
            export VIRTUE_CAPTURE_INTERVAL_SECONDS=120
            VIRTUE_BATCH_WINDOW_SECONDS="900"
            "#,
        );

        assert_eq!(
            parsed.get("VIRTUE_BASE_API_URL").map(String::as_str),
            Some("http://localhost:8787/")
        );
        assert_eq!(
            parsed
                .get("VIRTUE_CAPTURE_INTERVAL_SECONDS")
                .map(String::as_str),
            Some("120")
        );
        assert_eq!(
            parsed
                .get("VIRTUE_BATCH_WINDOW_SECONDS")
                .map(String::as_str),
            Some("900")
        );
    }
}
