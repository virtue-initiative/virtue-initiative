use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use virtue_core::Config;

pub const BASE_API_URL_ENV_VAR: &str = "VIRTUE_BASE_API_URL";
pub const CAPTURE_INTERVAL_SECONDS_ENV_VAR: &str = "VIRTUE_CAPTURE_INTERVAL_SECONDS";
pub const BATCH_WINDOW_SECONDS_ENV_VAR: &str = "VIRTUE_BATCH_WINDOW_SECONDS";

const DEFAULT_BASE_API_URL: &str = "https://api.virtueinitiative.org";
const DEFAULT_CAPTURE_INTERVAL_SECONDS: u64 = 300;
const MIN_CAPTURE_INTERVAL_SECONDS: u64 = 15;
const DEFAULT_BATCH_WINDOW_SECONDS: u64 = 3600;

#[derive(Clone, Debug)]
pub struct ClientPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub client_state_file: PathBuf,
    pub lifecycle_state_file: PathBuf,
}

impl ClientPaths {
    pub fn discover() -> Result<Self> {
        let config_root = dirs::config_dir().context("failed to resolve config directory")?;
        let data_root = dirs::data_dir().context("failed to resolve data directory")?;

        let config_dir = config_root.join("virtue");
        let data_dir = data_root.join("virtue");

        Ok(Self {
            state_dir: config_dir.join("core"),
            client_state_file: config_dir.join("client_state.json"),
            lifecycle_state_file: data_dir.join("lifecycle_state.json"),
            config_dir,
            data_dir,
        })
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ClientState {
    pub backend_hint: Option<CaptureBackendHint>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureBackendHint {
    Wayland,
    X11,
}

pub fn load_state(path: &Path) -> Result<ClientState> {
    if !path.exists() {
        return Ok(ClientState::default());
    }

    let raw = fs::read(path).with_context(|| format!("failed reading {}", path.display()))?;
    if raw.is_empty() {
        return Ok(ClientState::default());
    }

    let parsed = serde_json::from_slice::<ClientState>(&raw)
        .with_context(|| format!("failed parsing {}", path.display()))?;
    Ok(parsed)
}

pub fn save_state(path: &Path, state: &ClientState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let tmp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(state).context("failed serializing state")?;
    fs::write(&tmp, bytes).with_context(|| format!("failed writing {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed replacing {} with {}", path.display(), tmp.display()))?;

    Ok(())
}

pub fn build_core_config(paths: &ClientPaths) -> Config {
    let device_name = hostname::get()
        .ok()
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "linux-device".to_string());

    Config::new(
        resolve_base_api_url(),
        device_name,
        "linux",
        paths.state_dir.clone(),
        Duration::from_secs(resolve_capture_interval_seconds()),
        Duration::from_secs(resolve_batch_window_seconds()),
    )
}

pub fn resolve_base_api_url() -> String {
    std::env::var(BASE_API_URL_ENV_VAR)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_API_URL.to_string())
}

pub fn resolve_capture_interval_seconds() -> u64 {
    std::env::var(CAPTURE_INTERVAL_SECONDS_ENV_VAR)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(clamp_capture_interval_seconds)
        .unwrap_or(DEFAULT_CAPTURE_INTERVAL_SECONDS)
}

pub fn resolve_batch_window_seconds() -> u64 {
    std::env::var(BATCH_WINDOW_SECONDS_ENV_VAR)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(clamp_batch_window_seconds)
        .unwrap_or(DEFAULT_BATCH_WINDOW_SECONDS)
}

pub fn clamp_capture_interval_seconds(value: u64) -> u64 {
    value.max(MIN_CAPTURE_INTERVAL_SECONDS)
}

pub fn clamp_batch_window_seconds(value: u64) -> u64 {
    value.max(1)
}
