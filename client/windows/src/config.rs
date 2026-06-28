use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use virtue_core::Config;

const DEFAULT_BASE_API_URL: &str = virtue_core::DEFAULT_API_BASE_URL;
const DEFAULT_CAPTURE_INTERVAL_SECONDS: u64 = 300;
const DEFAULT_BATCH_WINDOW_SECONDS: u64 = 3600;

#[derive(Clone, Debug)]
pub struct ClientPaths {
    pub base_dir: PathBuf,
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub runtime_config_file: PathBuf,
    pub ui_state_file: PathBuf,
    pub log_file: PathBuf,
}

impl ClientPaths {
    pub fn discover() -> Result<Self> {
        let program_data = std::env::var_os("PROGRAMDATA")
            .context("PROGRAMDATA environment variable is not set")?;
        Ok(Self::from_base_dir(
            PathBuf::from(program_data).join("Virtue"),
        ))
    }

    pub fn from_base_dir(base_dir: PathBuf) -> Self {
        let config_dir = base_dir.join("config");
        let data_dir = base_dir.join("data");
        Self::from_config_and_data_dirs(config_dir, data_dir)
    }

    pub fn from_config_and_data_dirs(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        let base_dir = config_dir
            .parent()
            .map(Path::to_path_buf)
            .filter(|parent| data_dir.parent() == Some(parent.as_path()))
            .unwrap_or_else(|| config_dir.clone());

        Self {
            state_dir: data_dir.clone(),
            runtime_config_file: config_dir.join("config.json"),
            ui_state_file: config_dir.join("ui_state.json"),
            log_file: data_dir.join("service.log"),
            base_dir,
            config_dir,
            data_dir,
        }
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.config_dir)
            .with_context(|| format!("failed to create {}", self.config_dir.display()))?;
        fs::create_dir_all(&self.data_dir)
            .with_context(|| format!("failed to create {}", self.data_dir.display()))?;
        fs::create_dir_all(&self.state_dir)
            .with_context(|| format!("failed to create {}", self.state_dir.display()))?;
        Ok(())
    }
}

/// Default device name used at registration: the system hostname, or
/// `"windows-device"` if it can't be resolved.
pub fn default_device_name() -> String {
    hostname::get()
        .ok()
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "windows-device".to_string())
}

pub fn build_core_config(paths: &ClientPaths) -> Config {
    Config::new(
        DEFAULT_BASE_API_URL,
        default_device_name(),
        "windows",
        paths.state_dir.clone(),
        Some(paths.runtime_config_file.clone()),
        Duration::from_secs(DEFAULT_CAPTURE_INTERVAL_SECONDS),
        Duration::from_secs(DEFAULT_BATCH_WINDOW_SECONDS),
    )
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeConfigOverrides {
    #[serde(alias = "apiBaseUrl")]
    pub api_base_url: Option<String>,
    #[serde(alias = "captureIntervalSeconds")]
    pub capture_interval_seconds: Option<u64>,
    #[serde(alias = "batchWindowSeconds")]
    pub batch_window_seconds: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedRuntimeConfig {
    pub api_base_url: String,
    pub capture_interval_seconds: u64,
    pub batch_window_seconds: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientState {
    pub email: Option<String>,
}

pub fn load_state(path: &Path) -> Result<ClientState> {
    if !path.exists() {
        return Ok(ClientState::default());
    }

    let raw = fs::read(path).with_context(|| format!("failed reading {}", path.display()))?;
    if raw.is_empty() {
        return Ok(ClientState::default());
    }

    serde_json::from_slice(&raw).with_context(|| format!("failed parsing {}", path.display()))
}

pub fn save_state(path: &Path, state: &ClientState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let tmp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(state).context("failed serializing state")?;
    fs::write(&tmp, bytes).with_context(|| format!("failed writing {}", tmp.display()))?;
    replace_file(&tmp, path)?;

    Ok(())
}

pub fn load_runtime_overrides(path: &Path) -> Result<RuntimeConfigOverrides> {
    if !path.exists() {
        return Ok(RuntimeConfigOverrides::default());
    }

    let raw = fs::read(path).with_context(|| format!("failed reading {}", path.display()))?;
    if raw.is_empty() {
        return Ok(RuntimeConfigOverrides::default());
    }

    serde_json::from_slice(&raw).with_context(|| format!("failed parsing {}", path.display()))
}

pub fn save_runtime_overrides(path: &Path, config: &RuntimeConfigOverrides) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let tmp = path.with_extension("tmp");
    let bytes =
        serde_json::to_vec_pretty(config).context("failed serializing runtime overrides")?;
    fs::write(&tmp, bytes).with_context(|| format!("failed writing {}", tmp.display()))?;
    replace_file(&tmp, path)?;

    Ok(())
}

fn replace_file(tmp: &Path, destination: &Path) -> Result<()> {
    if destination.exists() {
        fs::remove_file(destination)
            .with_context(|| format!("failed removing {}", destination.display()))?;
    }

    fs::rename(tmp, destination).with_context(|| {
        format!(
            "failed replacing {} with {}",
            destination.display(),
            tmp.display()
        )
    })?;

    Ok(())
}

pub fn resolved_runtime_config(paths: &ClientPaths) -> Result<ResolvedRuntimeConfig> {
    let mut config = build_core_config(paths);
    config.refresh_from_runtime_file()?;

    Ok(ResolvedRuntimeConfig {
        api_base_url: config.api_base_url,
        capture_interval_seconds: config.screenshot_interval.as_secs(),
        batch_window_seconds: config.batch_interval.as_secs(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_runtime_overrides_replaces_existing_file() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let path = temp_dir.path().join("config.json");

        save_runtime_overrides(
            &path,
            &RuntimeConfigOverrides {
                api_base_url: Some("https://example.com".to_string()),
                capture_interval_seconds: Some(30),
                batch_window_seconds: Some(60),
            },
        )
        .expect("initial save succeeds");

        save_runtime_overrides(
            &path,
            &RuntimeConfigOverrides {
                api_base_url: Some("https://example.org".to_string()),
                capture_interval_seconds: Some(45),
                batch_window_seconds: Some(90),
            },
        )
        .expect("replacement save succeeds");

        let saved = load_runtime_overrides(&path).expect("load saved runtime overrides");
        assert_eq!(saved.api_base_url.as_deref(), Some("https://example.org"));
        assert_eq!(saved.capture_interval_seconds, Some(45));
        assert_eq!(saved.batch_window_seconds, Some(90));
    }

    #[test]
    fn load_runtime_overrides_accepts_camel_case_keys() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let path = temp_dir.path().join("config.json");
        fs::write(
            &path,
            r#"{
  "apiBaseUrl": "https://dev.example.com",
  "captureIntervalSeconds": 30,
  "batchWindowSeconds": 60
}"#,
        )
        .expect("write config");

        let saved = load_runtime_overrides(&path).expect("load saved runtime overrides");
        assert_eq!(
            saved.api_base_url.as_deref(),
            Some("https://dev.example.com")
        );
        assert_eq!(saved.capture_interval_seconds, Some(30));
        assert_eq!(saved.batch_window_seconds, Some(60));
    }

    #[test]
    fn save_runtime_overrides_uses_core_runtime_file_keys() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let path = temp_dir.path().join("config.json");

        save_runtime_overrides(
            &path,
            &RuntimeConfigOverrides {
                api_base_url: Some("https://example.com".to_string()),
                capture_interval_seconds: Some(30),
                batch_window_seconds: Some(60),
            },
        )
        .expect("save succeeds");

        let raw = fs::read_to_string(&path).expect("read saved runtime overrides");
        assert!(raw.contains("api_base_url"));
        assert!(raw.contains("capture_interval_seconds"));
        assert!(raw.contains("batch_window_seconds"));
    }
}
