use anyhow::{Context, Result};
use virtue_core::storage::FileStateStore;
use virtue_core::{CoreError, CoreResult, MonitorService, PlatformHooks, Screenshot};

use crate::config::{ClientPaths, ClientState, build_core_config, load_state, save_state};

#[derive(Clone)]
struct SessionPlatformHooks;

impl PlatformHooks for SessionPlatformHooks {
    fn take_screenshot(&self) -> CoreResult<Screenshot> {
        Err(CoreError::CommandFailed(
            "screenshot capture is unavailable in auth session".to_string(),
        ))
    }

    fn get_time_utc_ms(&self) -> CoreResult<i64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|err| CoreError::CommandFailed(err.to_string()))?;
        i64::try_from(now.as_millis()).map_err(|_| CoreError::InvalidState("system clock overflow"))
    }
}

#[derive(Clone)]
pub struct SessionManager {
    pub paths: ClientPaths,
}

#[derive(Clone, Debug)]
pub struct SessionStatus {
    pub logged_in: bool,
    pub device_id: Option<String>,
    pub email: Option<String>,
}

impl SessionManager {
    pub fn new() -> Result<Self> {
        let paths = ClientPaths::discover()?;
        paths.ensure_dirs()?;
        Ok(Self { paths })
    }

    pub fn status(&self) -> Result<SessionStatus> {
        let state = load_state(&self.paths.ui_state_file)?;
        let store = FileStateStore::new(&self.paths.state_dir)?;
        let auth = store.load_auth_state()?;

        Ok(SessionStatus {
            logged_in: auth.device_credentials.is_some(),
            device_id: auth
                .device_credentials
                .as_ref()
                .map(|device| device.device_id.clone()),
            email: state.email,
        })
    }

    pub fn login_blocking(&self, email: &str, password: &str, device_name: &str) -> Result<String> {
        let mut service =
            MonitorService::setup(build_core_config(&self.paths), SessionPlatformHooks)?;
        let result = service.login(email, password).context("login failed")?;

        save_state(
            &self.paths.ui_state_file,
            &ClientState {
                email: Some(email.to_string()),
            },
        )?;

        Ok(result
            .device
            .as_ref()
            .map(|device| device.device_id.clone())
            .unwrap_or_else(|| device_name.to_string()))
    }

    pub fn logout_blocking(&self) -> Result<()> {
        let mut service =
            MonitorService::setup(build_core_config(&self.paths), SessionPlatformHooks)?;
        service.logout()?;

        save_state(&self.paths.ui_state_file, &ClientState { email: None })?;
        Ok(())
    }
}
