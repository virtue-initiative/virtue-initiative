use serde::{Deserialize, Serialize};

use crate::api::ApiTransport;
use crate::error::CoreResult;
use crate::model::{AuthState, Redacted};
use crate::module::screenshot::{self, ScreenshotState};
use crate::module::upload::UploadState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequested {
    pub email: String,
    pub password: Redacted<String>,
    /// Optional device-name override chosen by the user at login. When `Some`
    /// and non-empty, it takes precedence over the construction-time device
    /// name (hostname / OS device name).
    #[serde(default)]
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResult {
    pub success: bool,
    pub error: Option<String>,
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoutRequested;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoutResult {
    pub success: bool,
    pub error: Option<String>,
}

/// Notification-only marker pushed to connected controllers whenever the
/// daemon transitions to logged-out — whether from an explicit `logout()`
/// call, an implicit revoke of a stale session during `login()`, or a
/// server-forced logout (401/404 on a batch upload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logout;

/// Logs in, revoking any existing device session first (a login while
/// another session is still active would otherwise leave the old device row
/// active on the server forever). On success, updates `auth`, enables
/// screenshot capture, and seeds `upload` with the fresh credentials/settings/
/// hash token. Returns the new device id.
///
/// Returns `true` in addition to the result to tell the caller whether an
/// existing session was revoked (so it can broadcast `Logout` to other
/// connected controllers).
#[allow(clippy::too_many_arguments)]
pub fn login<A: ApiTransport>(
    auth: &mut AuthState,
    screenshot: &mut ScreenshotState,
    upload: &mut UploadState,
    api: &A,
    device_name: &str,
    platform_name: &str,
    email: &str,
    password: &str,
    device_name_override: Option<&str>,
    now_ms: i64,
) -> (CoreResult<String>, bool) {
    let mut revoked = false;
    if let Some(creds) = auth.device_credentials.take() {
        let _ = api.logout(&creds.refresh_token);
        revoked = true;
    }

    let resolved_name = device_name_override
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(device_name);

    match api.register_device(email, password, resolved_name, platform_name) {
        Ok(registered) => {
            let device_id = registered.credentials.device_id.clone();
            auth.device_credentials = Some(registered.credentials.clone());
            upload.reset_for_login();
            upload.device_credentials = Some(registered.credentials);
            upload.settings = Some(registered.settings);
            upload.hash_token_cache = Some((registered.hash_token, now_ms));
            screenshot::enable(screenshot);
            (Ok(device_id), revoked)
        }
        Err(err) => (Err(err), revoked),
    }
}

/// Logs out (best-effort server-side revoke), clears auth/upload state, and
/// disables screenshot capture. Returns whether there was a session to
/// revoke (so the caller can decide whether a `Logout` broadcast is
/// warranted).
pub fn logout<A: ApiTransport>(
    auth: &mut AuthState,
    screenshot: &mut ScreenshotState,
    upload: &mut UploadState,
    api: &A,
) -> bool {
    let had_session = auth.device_credentials.is_some();
    if let Some(creds) = auth.device_credentials.take() {
        let _ = api.logout(&creds.refresh_token);
    }
    upload.reset_for_logout();
    screenshot::disable(screenshot);
    had_session
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MockApiClient;

    #[test]
    fn login_success_sets_state_and_enables_screenshots() {
        let mut auth = AuthState::default();
        let mut screenshot = ScreenshotState::default();
        let mut upload = UploadState::default();
        let api = MockApiClient::new();

        let (result, revoked) = login(
            &mut auth,
            &mut screenshot,
            &mut upload,
            &api,
            "test-device",
            "test-platform",
            "alice@example.org",
            "secret",
            None,
            1_000,
        );
        assert!(result.is_ok());
        assert!(!revoked, "first login has nothing to revoke");
        assert!(auth.device_credentials.is_some());
        assert!(upload.device_credentials.is_some());
        assert!(upload.settings.is_some());
        assert!(screenshot.enabled);
    }

    #[test]
    fn login_while_authenticated_revokes_previous_session_first() {
        let mut auth = AuthState::default();
        let mut screenshot = ScreenshotState::default();
        let mut upload = UploadState::default();
        let api = MockApiClient::new();

        let _ = login(
            &mut auth,
            &mut screenshot,
            &mut upload,
            &api,
            "d",
            "p",
            "alice@example.org",
            "secret",
            None,
            1_000,
        );
        let (result, revoked) = login(
            &mut auth,
            &mut screenshot,
            &mut upload,
            &api,
            "d",
            "p",
            "bob@example.org",
            "secret2",
            None,
            2_000,
        );
        assert!(result.is_ok());
        assert!(revoked);
        assert_eq!(api.state().logout_calls.len(), 1);
    }

    #[test]
    fn login_uses_device_name_override_when_present() {
        let mut auth = AuthState::default();
        let mut screenshot = ScreenshotState::default();
        let mut upload = UploadState::default();
        let api = MockApiClient::new();
        let _ = login(
            &mut auth,
            &mut screenshot,
            &mut upload,
            &api,
            "test-device",
            "p",
            "alice@example.org",
            "secret",
            Some("  My Laptop  "),
            1_000,
        );
        assert_eq!(api.state().register_device_calls[0].name, "My Laptop");
    }

    #[test]
    fn login_failure_leaves_state_logged_out() {
        let mut auth = AuthState::default();
        let mut screenshot = ScreenshotState::default();
        let mut upload = UploadState::default();
        let api = MockApiClient::new();
        api.program_register_device(Err(crate::error::CoreError::InvalidState("bad creds")));
        let (result, _) = login(
            &mut auth,
            &mut screenshot,
            &mut upload,
            &api,
            "d",
            "p",
            "alice@example.org",
            "wrong",
            None,
            1_000,
        );
        assert!(result.is_err());
        assert!(auth.device_credentials.is_none());
        assert!(!screenshot.enabled);
    }

    #[test]
    fn logout_without_session_reports_nothing_to_revoke() {
        let mut auth = AuthState::default();
        let mut screenshot = ScreenshotState::default();
        let mut upload = UploadState::default();
        let api = MockApiClient::new();
        let revoked = logout(&mut auth, &mut screenshot, &mut upload, &api);
        assert!(!revoked);
        assert!(api.state().logout_calls.is_empty());
    }

    #[test]
    fn logout_with_session_revokes_and_disables_screenshots() {
        let mut auth = AuthState::default();
        let mut screenshot = ScreenshotState::default();
        let mut upload = UploadState::default();
        let api = MockApiClient::new();
        let _ = login(
            &mut auth,
            &mut screenshot,
            &mut upload,
            &api,
            "d",
            "p",
            "alice@example.org",
            "secret",
            None,
            1_000,
        );
        let revoked = logout(&mut auth, &mut screenshot, &mut upload, &api);
        assert!(revoked);
        assert_eq!(api.state().logout_calls.len(), 1);
        assert!(!screenshot.enabled);
        assert!(upload.device_credentials.is_none());
    }
}
