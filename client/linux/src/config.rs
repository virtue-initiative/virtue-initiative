use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use virtue_client_core::{DEFAULT_BATCH_WINDOW_SECONDS, DEFAULT_CAPTURE_INTERVAL_SECONDS};

#[derive(Clone, Debug)]
pub struct ClientPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_file: PathBuf,
    pub token_file: PathBuf,
    pub batch_buffer_file: PathBuf,
}

impl ClientPaths {
    pub fn discover() -> Result<Self> {
        let config_root = dirs::config_dir().context("failed to resolve config directory")?;
        let data_root = dirs::data_dir().context("failed to resolve data directory")?;

        let config_dir = config_root.join("virtue");
        let data_dir = data_root.join("virtue");

        Ok(Self {
            state_file: config_dir.join("client_state.json"),
            token_file: config_dir.join("token_store.json"),
            batch_buffer_file: data_dir.join("batch_buffer.json"),
            config_dir,
            data_dir,
        })
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.config_dir)
            .with_context(|| format!("failed to create {}", self.config_dir.display()))?;
        fs::create_dir_all(&self.data_dir)
            .with_context(|| format!("failed to create {}", self.data_dir.display()))?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientState {
    pub monitoring_enabled: bool,
    pub capture_interval_seconds: u64,
    /// How many seconds of captures to accumulate before uploading a batch.
    pub batch_window_seconds: u64,
    pub email: Option<String>,
    pub device_id: Option<String>,
    pub backend_hint: Option<CaptureBackendHint>,
    /// User ID used as PBKDF2 salt for E2EE key derivation.
    pub e2ee_user_id: Option<String>,
}

impl Default for ClientState {
    fn default() -> Self {
        Self {
            monitoring_enabled: false,
            capture_interval_seconds: DEFAULT_CAPTURE_INTERVAL_SECONDS,
            batch_window_seconds: DEFAULT_BATCH_WINDOW_SECONDS,
            email: None,
            device_id: None,
            backend_hint: None,
            e2ee_user_id: None,
        }
    }
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
