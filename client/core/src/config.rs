use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

use crate::error::CoreResult;

const MIN_CAPTURE_INTERVAL_SECONDS: u64 = 15;
const MIN_BATCH_INTERVAL_SECONDS: u64 = 1;

#[derive(Debug, Clone)]
pub struct Config {
    pub api_base_url: String,
    pub device_name: String,
    pub platform_name: String,
    pub state_dir: PathBuf,
    pub runtime_config_file: Option<PathBuf>,
    pub screenshot_interval: Duration,
    pub batch_interval: Duration,
    default_api_base_url: String,
    default_screenshot_interval: Duration,
    default_batch_interval: Duration,
}

impl Config {
    pub fn new(
        api_base_url: impl Into<String>,
        device_name: impl Into<String>,
        platform_name: impl Into<String>,
        state_dir: PathBuf,
        runtime_config_file: Option<PathBuf>,
        screenshot_interval: Duration,
        batch_interval: Duration,
    ) -> Self {
        let api_base_url = normalize_base_url(api_base_url.into());
        Self {
            api_base_url: api_base_url.clone(),
            device_name: device_name.into(),
            platform_name: platform_name.into(),
            state_dir,
            runtime_config_file,
            screenshot_interval,
            batch_interval,
            default_api_base_url: api_base_url,
            default_screenshot_interval: screenshot_interval,
            default_batch_interval: batch_interval,
        }
    }

    pub fn refresh_from_runtime_file(&mut self) -> CoreResult<()> {
        self.api_base_url = self.default_api_base_url.clone();
        self.screenshot_interval = self.default_screenshot_interval;
        self.batch_interval = self.default_batch_interval;

        let Some(path) = self.runtime_config_file.as_ref() else {
            return Ok(());
        };
        if !path.exists() {
            return Ok(());
        }

        let bytes = fs::read(path)?;
        if bytes.is_empty() {
            return Ok(());
        }

        let overrides: RuntimeConfigFile = serde_json::from_slice(&bytes)?;
        if let Some(api_base_url) = overrides.api_base_url {
            let normalized = normalize_base_url(api_base_url);
            if !normalized.is_empty() {
                self.api_base_url = normalized;
            }
        }
        if let Some(seconds) = overrides.capture_interval_seconds {
            self.screenshot_interval =
                Duration::from_secs(seconds.max(MIN_CAPTURE_INTERVAL_SECONDS));
        }
        if let Some(seconds) = overrides.batch_window_seconds {
            self.batch_interval = Duration::from_secs(seconds.max(MIN_BATCH_INTERVAL_SECONDS));
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct RuntimeConfigFile {
    api_base_url: Option<String>,
    capture_interval_seconds: Option<u64>,
    batch_window_seconds: Option<u64>,
}

fn normalize_base_url(value: String) -> String {
    value.trim().trim_end_matches('/').to_string()
}
