use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct ClientPaths {
    pub base_dir: PathBuf,
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_file: PathBuf,
    pub token_file: PathBuf,
    pub batch_buffer_file: PathBuf,
    pub service_env_file: PathBuf,
    pub log_file: PathBuf,
}

impl ClientPaths {
    pub fn discover() -> Result<Self> {
        let program_data = std::env::var_os("PROGRAMDATA")
            .context("PROGRAMDATA environment variable is not set")?;
        let base_dir = PathBuf::from(program_data).join("Virtue");
        let config_dir = base_dir.join("config");
        let data_dir = base_dir.join("data");

        Ok(Self {
            state_file: config_dir.join("client_state.json"),
            token_file: config_dir.join("token_store.json"),
            batch_buffer_file: data_dir.join("batch_buffer.json"),
            service_env_file: config_dir.join("service.dev.env"),
            log_file: data_dir.join("service.log"),
            base_dir,
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientState {
    pub monitoring_enabled: bool,
    pub device_id: Option<String>,
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
