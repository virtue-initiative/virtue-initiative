use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

use crate::error::{CoreError, CoreResult};

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

        let overrides: RuntimeConfigFile =
            serde_json::from_slice(&bytes).map_err(|e| CoreError::SerdeContext {
                context: path.display().to_string(),
                source: e,
            })?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::time::Duration;

    fn base_config(runtime_file: Option<std::path::PathBuf>) -> Config {
        Config::new(
            "https://default.example.com",
            "test-device",
            "test-platform",
            env::temp_dir(),
            runtime_file,
            Duration::from_secs(60),
            Duration::from_secs(60),
        )
    }

    #[test]
    fn refresh_with_no_runtime_file_leaves_defaults() {
        let mut config = base_config(None);
        config.refresh_from_runtime_file().expect("no error");
        assert_eq!(config.api_base_url, "https://default.example.com");
        assert_eq!(config.screenshot_interval, Duration::from_secs(60));
    }

    #[test]
    fn refresh_with_missing_file_leaves_defaults() {
        let path = env::temp_dir().join(format!(
            "virtue-core-cfg-missing-{}.json",
            std::process::id()
        ));
        let mut config = base_config(Some(path));
        config.refresh_from_runtime_file().expect("no error");
        assert_eq!(config.api_base_url, "https://default.example.com");
    }

    #[test]
    fn refresh_with_empty_file_leaves_defaults() {
        let path =
            env::temp_dir().join(format!("virtue-core-cfg-empty-{}.json", std::process::id()));
        fs::write(&path, b"").expect("write empty file");
        let mut config = base_config(Some(path.clone()));
        config.refresh_from_runtime_file().expect("no error");
        assert_eq!(config.api_base_url, "https://default.example.com");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn refresh_overrides_api_base_url() {
        let path = env::temp_dir().join(format!("virtue-core-cfg-url-{}.json", std::process::id()));
        fs::write(&path, br#"{"api_base_url":"https://new.example.com/"}"#).expect("write file");
        let mut config = base_config(Some(path.clone()));
        config.refresh_from_runtime_file().expect("no error");
        assert_eq!(config.api_base_url, "https://new.example.com");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn refresh_clamps_capture_interval_below_minimum() {
        let path = env::temp_dir().join(format!(
            "virtue-core-cfg-interval-{}.json",
            std::process::id()
        ));
        fs::write(&path, br#"{"capture_interval_seconds":5}"#).expect("write file");
        let mut config = base_config(Some(path.clone()));
        config.refresh_from_runtime_file().expect("no error");
        assert_eq!(
            config.screenshot_interval,
            Duration::from_secs(MIN_CAPTURE_INTERVAL_SECONDS)
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn refresh_with_invalid_json_returns_error() {
        let path = env::temp_dir().join(format!(
            "virtue-core-cfg-badjson-{}.json",
            std::process::id()
        ));
        fs::write(&path, b"not valid json at all").expect("write file");
        let mut config = base_config(Some(path.clone()));
        let result = config.refresh_from_runtime_file();
        assert!(result.is_err());
        let _ = fs::remove_file(path);
    }
}
