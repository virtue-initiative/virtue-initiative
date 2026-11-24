use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use virtue_core::Config;

const DEFAULT_BASE_API_URL: &str = virtue_core::DEFAULT_API_BASE_URL;

#[derive(Clone, Debug)]
pub struct ClientPaths {
    pub base_dir: PathBuf,
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub ui_state_file: PathBuf,
    pub log_dir: PathBuf,
}

/// Set by the WinUI app before it calls into this library, to the base directory it chose
/// (CLIENT-002): the package's own `LocalState\Virtue` for a packaged install, so the OS deletes
/// the state at uninstall. When unset (an unpackaged or dev run, the CI runner, tests) the base
/// directory is `%PROGRAMDATA%\Virtue`.
pub const BASE_DIR_ENV: &str = "VIRTUE_WINDOWS_BASE_DIR";

impl ClientPaths {
    pub fn discover() -> Result<Self> {
        Ok(Self::from_base_dir(Self::discover_base_dir(
            std::env::var_os(BASE_DIR_ENV),
            std::env::var_os("PROGRAMDATA"),
        )?))
    }

    fn discover_base_dir(
        base_dir: Option<std::ffi::OsString>,
        program_data: Option<std::ffi::OsString>,
    ) -> Result<PathBuf> {
        if let Some(base_dir) = base_dir.filter(|value| !value.is_empty()) {
            return Ok(PathBuf::from(base_dir));
        }
        let program_data = program_data.context("PROGRAMDATA environment variable is not set")?;
        Ok(PathBuf::from(program_data).join("Virtue"))
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
            ui_state_file: config_dir.join("ui_state.json"),
            log_dir: data_dir.join("logs"),
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
        fs::create_dir_all(&self.log_dir)
            .with_context(|| format!("failed to create {}", self.log_dir.display()))?;
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
        Duration::from_secs(virtue_core::default_capture_interval_seconds()),
        Duration::from_secs(virtue_core::default_batch_window_seconds()),
    )
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientState {
    pub email: Option<String>,
    /// The self-hosted server address last signed in to, kept across sign-out
    /// to prefill the next sign-in. `None` is the default server (CLIENT-034).
    pub server: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    // CLIENT-002: the base directory the app hands over wins over the ProgramData default.
    #[test]
    fn base_dir_from_the_app_wins_over_program_data() {
        let base = ClientPaths::discover_base_dir(
            Some(OsString::from(
                r"C:\Users\u\AppData\Local\Packages\pkg\LocalState\Virtue",
            )),
            Some(OsString::from(r"C:\ProgramData")),
        )
        .expect("base dir");
        assert_eq!(
            base,
            PathBuf::from(r"C:\Users\u\AppData\Local\Packages\pkg\LocalState\Virtue")
        );
    }

    // CLIENT-002: an unpackaged run, with no base directory handed over, uses ProgramData.
    #[test]
    fn without_a_base_dir_falls_back_to_program_data() {
        for unset in [None, Some(OsString::new())] {
            let base = ClientPaths::discover_base_dir(unset, Some(OsString::from("pd")))
                .expect("base dir");
            assert_eq!(base, PathBuf::from("pd").join("Virtue"));
        }
    }
}
