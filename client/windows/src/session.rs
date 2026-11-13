use anyhow::{Context, Result};
use virtue_core::CoreError;

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
        let device_id = resident_monitor::app_login(email, password, device_name)
            .map_err(|err| anyhow::anyhow!(login_error_message(&err)))?;

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

const BAD_CREDENTIALS: &str = "Sign in failed. Check your email and password and try again.";
const UNREACHABLE: &str = "Can't reach Virtue. Check your internet connection and try again.";

/// CLIENT-004: the plain sign-in failure text shown instead of raw core text.
fn login_error_message(err: &anyhow::Error) -> String {
    match err.downcast_ref::<CoreError>() {
        Some(CoreError::HttpStatus {
            status: 400 | 401 | 403,
            ..
        }) => BAD_CREDENTIALS.to_string(),
        Some(CoreError::Http(_) | CoreError::Io(_)) => UNREACHABLE.to_string(),
        // Not a core failure (for example "monitoring is not running"): CLIENT-015 wording.
        None => format!("{err:#}"),
        Some(other) => with_period(&format!("Sign in failed. {other}")),
    }
}

fn with_period(text: &str) -> String {
    if text.ends_with(['.', '!', '?']) {
        text.to_string()
    } else {
        format!("{text}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    fn msg(err: CoreError) -> String {
        login_error_message(&anyhow::Error::from(err))
    }

    // CLIENT-004: a rejected email or password reads as a plain sign-in failure.
    #[test]
    fn rejected_credentials_show_the_plain_sign_in_failure() {
        let err = CoreError::HttpStatus {
            status: 401,
            message: "Invalid email or password".to_string(),
        };
        assert_eq!(msg(err), BAD_CREDENTIALS);
    }

    // CLIENT-004: a request that never reaches the server tells the user to check their connection.
    #[test]
    fn unreachable_server_shows_the_connection_message() {
        let err = CoreError::Io(io::Error::other("No such host is known."));
        assert_eq!(msg(err), UNREACHABLE);
    }

    // CLIENT-004: any other failure keeps its detail, with a terminal period.
    #[test]
    fn other_failures_get_a_prefix_and_a_period() {
        let err = CoreError::HttpStatus {
            status: 500,
            message: "Server error".to_string(),
        };
        assert_eq!(msg(err), "Sign in failed. Server error.");
    }
}
