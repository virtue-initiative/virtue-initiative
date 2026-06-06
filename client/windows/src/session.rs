use anyhow::{Context, Result};
use virtue_core::ControllerClient;

use crate::config::{ClientPaths, ClientState, load_state, save_state};

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
        let sock = self.paths.state_dir.join("daemon.sock");
        let service_status = ControllerClient::connect(&sock)
            .ok()
            .and_then(|mut c| c.get_status().ok());

        Ok(SessionStatus {
            logged_in: service_status.as_ref().is_some_and(|s| s.is_authenticated),
            device_id: service_status.and_then(|s| s.device_id),
            email: state.email,
        })
    }

    pub fn login_blocking(
        &self,
        email: &str,
        password: &str,
        _device_name: &str,
    ) -> Result<String> {
        let sock = self.paths.state_dir.join("daemon.sock");
        let mut client = ControllerClient::connect(&sock)
            .context("failed to connect to daemon (is monitoring running?)")?;
        let device_id = client.login(email, password).context("login failed")?;

        save_state(
            &self.paths.ui_state_file,
            &ClientState {
                email: Some(email.to_string()),
            },
        )?;

        Ok(device_id)
    }

    pub fn logout_blocking(&self) -> Result<()> {
        let sock = self.paths.state_dir.join("daemon.sock");
        let mut client = ControllerClient::connect(&sock)
            .context("failed to connect to daemon (is monitoring running?)")?;
        client.logout().context("logout failed")?;

        save_state(&self.paths.ui_state_file, &ClientState { email: None })?;
        Ok(())
    }
}
