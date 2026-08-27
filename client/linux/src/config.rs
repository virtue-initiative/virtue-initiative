use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use virtue_core::ipc::ClientController;
use virtue_core::module::status;
use virtue_core::{Config, DaemonState, ServiceStatus, load_state};

const DEFAULT_BASE_API_URL: &str = virtue_core::DEFAULT_API_BASE_URL;

/// Set at build time by passing `VIRTUE_INSTANCE=<name>` to cargo. Controls
/// which XDG subdirectory and systemd service name this binary uses.
pub const INSTANCE: Option<&str> = option_env!("VIRTUE_INSTANCE");

#[derive(Clone, Debug)]
pub struct ClientPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
}

impl ClientPaths {
    pub fn discover() -> Result<Self> {
        let config_root = xdg_base_dir("XDG_CONFIG_HOME", ".config")
            .context("failed to resolve config directory")?;
        let state_root = xdg_base_dir("XDG_STATE_HOME", ".local/state")
            .context("failed to resolve state directory")?;
        Ok(Self::from_roots(config_root, state_root, INSTANCE))
    }

    fn from_roots(config_root: PathBuf, state_root: PathBuf, instance: Option<&str>) -> Self {
        let dir_name = match instance {
            Some(n) if !n.is_empty() => format!("virtue-{n}"),
            _ => "virtue".to_string(),
        };
        let config_dir = config_root.join(&dir_name);
        let data_dir = state_root.join(&dir_name);
        Self {
            state_dir: data_dir.clone(),
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
/// `"linux-device"` if it can't be resolved.
pub fn default_device_name() -> String {
    hostname::get()
        .ok()
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "linux-device".to_string())
}

pub fn build_core_config(paths: &ClientPaths) -> Config {
    Config::new(
        DEFAULT_BASE_API_URL,
        default_device_name(),
        "linux",
        paths.state_dir.clone(),
        Duration::from_secs(virtue_core::default_capture_interval_seconds()),
        Duration::from_secs(virtue_core::default_batch_window_seconds()),
    )
}

/// The current service status: live from the daemon over IPC when it's
/// reachable, or computed from its last state persisted to disk otherwise
/// (e.g. the systemd service is stopped) — the daemon process not running is
/// not the same as the user being logged out. Either way this goes through
/// `virtue_core::module::status::build`, the same pure function the daemon
/// itself uses, so the two paths can't drift apart. See CORE-010.
pub fn load_service_status(paths: &ClientPaths) -> Result<ServiceStatus> {
    let sock = paths.state_dir.join("daemon.sock");
    if let Ok(mut client) = ClientController::connect(&sock)
        && let Ok(status) = client.get_status()
    {
        return Ok(status);
    }
    let state_path = paths.state_dir.join("event_state.json");
    let state: DaemonState = load_state(&state_path)?;
    Ok(status::build(&state, &build_core_config(paths), false))
}

fn xdg_base_dir(env_name: &str, fallback_suffix: &str) -> Result<PathBuf> {
    if let Some(value) = std::env::var_os(env_name).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(value));
    }

    let home = dirs::home_dir().context("failed to resolve home directory")?;
    Ok(home.join(fallback_suffix))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use virtue_core::{AuthState, DeviceCredentials};

    use super::{ClientPaths, load_service_status};

    #[test]
    fn state_dir_is_under_state_root() {
        let paths =
            ClientPaths::from_roots(PathBuf::from("/tmp/cfg"), PathBuf::from("/tmp/state"), None);
        assert_eq!(paths.state_dir, PathBuf::from("/tmp/state/virtue"));
        assert_eq!(paths.data_dir, PathBuf::from("/tmp/state/virtue"));
    }

    #[test]
    fn config_dir_is_under_config_root() {
        let paths = ClientPaths::from_roots(
            PathBuf::from("/home/user/.config"),
            PathBuf::from("/home/user/.local/state"),
            None,
        );
        assert_eq!(paths.config_dir, PathBuf::from("/home/user/.config/virtue"));
    }

    #[test]
    fn fallback_paths_follow_xdg_spec_conventions() {
        let home = PathBuf::from("/home/testuser");
        let paths = ClientPaths::from_roots(home.join(".config"), home.join(".local/state"), None);
        assert_eq!(
            paths.state_dir,
            PathBuf::from("/home/testuser/.local/state/virtue")
        );
        assert_eq!(
            paths.config_dir,
            PathBuf::from("/home/testuser/.config/virtue")
        );
    }

    #[test]
    fn instance_name_is_appended_to_dir_name() {
        let paths = ClientPaths::from_roots(
            PathBuf::from("/home/user/.config"),
            PathBuf::from("/home/user/.local/state"),
            Some("dev"),
        );
        assert_eq!(
            paths.config_dir,
            PathBuf::from("/home/user/.config/virtue-dev")
        );
        assert_eq!(
            paths.state_dir,
            PathBuf::from("/home/user/.local/state/virtue-dev")
        );
    }

    #[test]
    fn empty_instance_falls_back_to_default_dir_name() {
        let paths = ClientPaths::from_roots(
            PathBuf::from("/home/user/.config"),
            PathBuf::from("/home/user/.local/state"),
            Some(""),
        );
        assert_eq!(paths.config_dir, PathBuf::from("/home/user/.config/virtue"));
    }

    /// A fresh, unique scratch `ClientPaths` per test, pointed at a state
    /// dir with no `daemon.sock` in it — `load_service_status` can never
    /// reach a live daemon here, so it always exercises the disk fallback.
    fn scratch_paths(test_name: &str) -> ClientPaths {
        let dir = std::env::temp_dir().join(format!(
            "virtue-linux-config-test-{test_name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch state dir");
        ClientPaths {
            config_dir: dir.clone(),
            data_dir: dir.clone(),
            state_dir: dir,
        }
    }

    #[test]
    fn status_defaults_to_logged_out_when_daemon_is_unreachable_and_never_ran() {
        let paths = scratch_paths("missing-file");
        let status = load_service_status(&paths).unwrap();
        assert!(!status.is_authenticated);
        assert!(!status.is_running);
    }

    #[test]
    fn status_reflects_credentials_persisted_by_the_daemon_even_when_it_is_stopped() {
        let paths = scratch_paths("with-credentials");
        let persisted_auth = AuthState {
            device_credentials: Some(DeviceCredentials {
                device_id: "dev-123".to_string(),
                refresh_token: "refresh-abc".to_string(),
            }),
            account_email: Some("alice@example.org".to_string()),
        };
        let event_state = serde_json::json!({ "auth": persisted_auth });
        std::fs::write(
            paths.state_dir.join("event_state.json"),
            serde_json::to_vec(&event_state).unwrap(),
        )
        .unwrap();

        let status = load_service_status(&paths).unwrap();
        assert!(status.is_authenticated);
        assert!(!status.is_running);
        assert_eq!(status.device_id.as_deref(), Some("dev-123"));
        assert_eq!(status.account_email.as_deref(), Some("alice@example.org"));
        // The advanced fields come from the compile-time config, so they are
        // populated even on the daemon-stopped path.
        assert!(!status.api_base_url.is_empty());
        assert!(status.capture_interval_seconds > 0);
    }
}
