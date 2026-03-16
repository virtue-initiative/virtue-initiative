use std::path::PathBuf;
use std::time::Duration;

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
            api_base_url: api_base_url.into(),
            device_name: device_name.into(),
            platform_name: platform_name.into(),
            state_dir,
            screenshot_interval,
            batch_interval,
        }
    }
}
