use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use virtue_core::Config;

const DEFAULT_BASE_API_URL: &str = "https://api.virtueinitiative.org";
const DEFAULT_CAPTURE_INTERVAL_SECONDS: u64 = 300;
const DEFAULT_BATCH_WINDOW_SECONDS: u64 = 3600;

#[derive(Clone, Debug)]
pub struct ClientPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub runtime_config_file: PathBuf,
}

impl ClientPaths {
    pub fn discover() -> Result<Self> {
        let config_root = xdg_base_dir("XDG_CONFIG_HOME", ".config")
            .context("failed to resolve config directory")?;
        let state_root = xdg_base_dir("XDG_STATE_HOME", ".local/state")
            .context("failed to resolve state directory")?;
        Ok(Self::from_roots(config_root, state_root))
    }

    fn from_roots(config_root: PathBuf, state_root: PathBuf) -> Self {
        let config_dir = config_root.join("virtue");
        let data_dir = state_root.join("virtue");
        Self {
            state_dir: data_dir.clone(),
            runtime_config_file: config_dir.join("config.json"),
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

pub fn build_core_config(paths: &ClientPaths) -> Config {
    let device_name = hostname::get()
        .ok()
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "linux-device".to_string());

    Config::new(
        DEFAULT_BASE_API_URL,
        device_name,
        "linux",
        paths.state_dir.clone(),
        Some(paths.runtime_config_file.clone()),
        Duration::from_secs(DEFAULT_CAPTURE_INTERVAL_SECONDS),
        Duration::from_secs(DEFAULT_BATCH_WINDOW_SECONDS),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::ClientPaths;

    #[test]
    fn state_dir_is_under_state_root() {
        let paths = ClientPaths::from_roots(PathBuf::from("/tmp/cfg"), PathBuf::from("/tmp/state"));
        assert_eq!(paths.state_dir, PathBuf::from("/tmp/state/virtue"));
        assert_eq!(paths.data_dir, PathBuf::from("/tmp/state/virtue"));
    }

    #[test]
    fn config_dir_and_runtime_file_are_under_config_root() {
        let paths = ClientPaths::from_roots(
            PathBuf::from("/home/user/.config"),
            PathBuf::from("/home/user/.local/state"),
        );
        assert_eq!(paths.config_dir, PathBuf::from("/home/user/.config/virtue"));
        assert_eq!(
            paths.runtime_config_file,
            PathBuf::from("/home/user/.config/virtue/config.json")
        );
    }

    #[test]
    fn fallback_paths_follow_xdg_spec_conventions() {
        let home = PathBuf::from("/home/testuser");
        let paths = ClientPaths::from_roots(home.join(".config"), home.join(".local/state"));
        assert_eq!(
            paths.state_dir,
            PathBuf::from("/home/testuser/.local/state/virtue")
        );
        assert_eq!(
            paths.config_dir,
            PathBuf::from("/home/testuser/.config/virtue")
        );
    }
}

fn xdg_base_dir(env_name: &str, fallback_suffix: &str) -> Result<PathBuf> {
    if let Some(value) = std::env::var_os(env_name).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(value));
    }

    let home = dirs::home_dir().context("failed to resolve home directory")?;
    Ok(home.join(fallback_suffix))
}
