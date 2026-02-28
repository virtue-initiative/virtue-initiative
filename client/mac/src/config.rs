use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use virtue_client_core::DEFAULT_CAPTURE_INTERVAL_SECONDS;

#[derive(Clone, Debug)]
pub struct ClientPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub launch_agents_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub state_file: PathBuf,
    pub token_file: PathBuf,
    pub queue_file: PathBuf,
    pub daemon_status_file: PathBuf,
    pub launch_agent_file: PathBuf,
}

impl ClientPaths {
    pub fn discover() -> Result<Self> {
        let config_root = dirs::config_dir().context("failed to resolve config directory")?;
        let data_root = dirs::data_dir().context("failed to resolve data directory")?;
        let home = dirs::home_dir().context("failed to resolve home directory")?;

        let config_dir = config_root.join("virtue");
        let data_dir = data_root.join("virtue");
        let launch_agents_dir = home.join("Library").join("LaunchAgents");
        let logs_dir = home.join("Library").join("Logs");

        Ok(Self {
            state_file: config_dir.join("mac_client_state.json"),
            token_file: config_dir.join("token_store.json"),
            queue_file: data_dir.join("upload_queue.json"),
            daemon_status_file: data_dir.join("mac_daemon_status.json"),
            launch_agent_file: launch_agents_dir.join("codes.anb.virtue.daemon.plist"),
            config_dir,
            data_dir,
            launch_agents_dir,
            logs_dir,
        })
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.config_dir)
            .with_context(|| format!("failed to create {}", self.config_dir.display()))?;
        fs::create_dir_all(&self.data_dir)
            .with_context(|| format!("failed to create {}", self.data_dir.display()))?;
        fs::create_dir_all(&self.launch_agents_dir)
            .with_context(|| format!("failed to create {}", self.launch_agents_dir.display()))?;
        fs::create_dir_all(&self.logs_dir)
            .with_context(|| format!("failed to create {}", self.logs_dir.display()))?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientState {
    pub monitoring_enabled: bool,
    pub capture_interval_seconds: u64,
    pub device_id: Option<String>,
    pub email: Option<String>,
}

impl Default for ClientState {
    fn default() -> Self {
        Self {
            monitoring_enabled: false,
            capture_interval_seconds: DEFAULT_CAPTURE_INTERVAL_SECONDS,
            device_id: None,
            email: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotPermissionStatus {
    #[default]
    Unknown,
    Granted,
    Missing,
}

impl ScreenshotPermissionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Granted => "granted",
            Self::Missing => "missing",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DaemonStatus {
    #[serde(default)]
    pub screenshot_permission: ScreenshotPermissionStatus,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
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

pub fn load_daemon_status(path: &Path) -> Result<DaemonStatus> {
    if !path.exists() {
        return Ok(DaemonStatus::default());
    }

    let raw = fs::read(path).with_context(|| format!("failed reading {}", path.display()))?;
    if raw.is_empty() {
        return Ok(DaemonStatus::default());
    }

    let parsed = serde_json::from_slice::<DaemonStatus>(&raw)
        .with_context(|| format!("failed parsing {}", path.display()))?;
    Ok(parsed)
}

pub fn save_daemon_status(path: &Path, status: &DaemonStatus) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let tmp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(status).context("failed serializing daemon status")?;
    fs::write(&tmp, bytes).with_context(|| format!("failed writing {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed replacing {} with {}", path.display(), tmp.display()))?;

    Ok(())
}
