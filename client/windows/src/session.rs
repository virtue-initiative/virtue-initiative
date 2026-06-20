use anyhow::{Context, Result};

use crate::config::{ClientPaths, ClientState, load_state, save_state};
use crate::resident_monitor;

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
        let snapshot = resident_monitor::status_snapshot();

        Ok(SessionStatus {
            logged_in: snapshot.logged_in,
            device_id: None,
            email: state.email,
        })
    }

    pub fn login_blocking(&self, email: &str, password: &str, device_name: &str) -> Result<String> {
        let device_id =
            resident_monitor::app_login(email, password, device_name).context("login failed")?;

        save_state(
            &self.paths.ui_state_file,
            &ClientState {
                email: Some(email.to_string()),
            },
        )?;

        Ok(device_id)
    }

    pub fn logout_blocking(&self) -> Result<()> {
        resident_monitor::app_logout().context("logout failed")?;
        save_state(&self.paths.ui_state_file, &ClientState { email: None })?;
        Ok(())
    }
}
