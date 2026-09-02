use anyhow::{Context, Result};

use virtue_core::api::DeviceCodeStart;
use virtue_core::module::auth::CodeLoginPoll;

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
            // The daemon is the authority on which device is registered; the
            // UI state file only remembers the email that was typed in.
            device_id: snapshot.core.as_ref().and_then(|s| s.device_id.clone()),
            email: snapshot
                .core
                .as_ref()
                .and_then(|s| s.account_email.clone())
                .or(state.email),
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

    /// CORE-020. Nothing is written to the UI state file here: no account is
    /// known until the pairing is approved, and the device's own poll is what
    /// learns the email (API-045).
    pub fn begin_code_login_blocking(&self, device_name: &str) -> Result<DeviceCodeStart> {
        resident_monitor::app_begin_code_login(device_name).context("could not start a code login")
    }

    /// CORE-021. On approval the account email comes back through the daemon's
    /// status, so the UI state file only records that a session now exists.
    pub fn poll_code_login_blocking(&self) -> Result<CodeLoginPoll> {
        let outcome =
            resident_monitor::app_poll_code_login().context("could not check the code")?;

        if matches!(outcome, CodeLoginPoll::Approved { .. }) {
            let email = resident_monitor::status_snapshot()
                .core
                .and_then(|status| status.account_email);
            save_state(&self.paths.ui_state_file, &ClientState { email })?;
        }

        Ok(outcome)
    }

    pub fn logout_blocking(&self) -> Result<()> {
        resident_monitor::app_logout().context("logout failed")?;
        save_state(&self.paths.ui_state_file, &ClientState { email: None })?;
        Ok(())
    }
}
