use std::path::PathBuf;
use std::time::Duration;

pub const DEFAULT_API_BASE_URL: &str = env!("VIRTUE_DEFAULT_API_URL");

pub fn default_capture_interval_seconds() -> u64 {
    env!("VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS")
        .parse()
        .expect("set by build.rs")
}

pub fn default_batch_window_seconds() -> u64 {
    env!("VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS")
        .parse()
        .expect("set by build.rs")
}

#[derive(Debug, Clone)]
pub struct Config {
    pub api_base_url: String,
    pub device_name: String,
    pub platform_name: String,
    pub state_dir: PathBuf,
    pub screenshot_interval: Duration,
    pub batch_interval: Duration,
}

impl Config {
    pub fn new(
        api_base_url: impl Into<String>,
        device_name: impl Into<String>,
        platform_name: impl Into<String>,
        state_dir: PathBuf,
        screenshot_interval: Duration,
        batch_interval: Duration,
    ) -> Self {
        Self {
            api_base_url: normalize_base_url(api_base_url.into()),
            device_name: device_name.into(),
            platform_name: platform_name.into(),
            state_dir,
            screenshot_interval,
            batch_interval,
        }
    }
}

fn normalize_base_url(value: String) -> String {
    value.trim().trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_base_url_trims_whitespace_and_trailing_slash() {
        assert_eq!(
            normalize_base_url("  https://example.com/ ".to_string()),
            "https://example.com"
        );
        assert_eq!(
            normalize_base_url("https://example.com///".to_string()),
            "https://example.com"
        );
        assert_eq!(
            normalize_base_url("https://example.com".to_string()),
            "https://example.com"
        );
    }
}
