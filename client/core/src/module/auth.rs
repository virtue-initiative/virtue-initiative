use crate::api::ApiTransport;
use crate::error::CoreResult;
use crate::model::AuthState;
use crate::module::screenshot::{self, ScreenshotState};
use crate::module::upload::UploadState;

/// Logs in, revoking any existing device session first (a login while
/// another session is still active would otherwise leave the old device row
/// active on the server forever). On success, updates `auth`, enables
/// screenshot capture, and seeds `upload` with the fresh credentials/settings/
/// hash token. Returns the new device id.
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
) -> CoreResult<String> {
    if let Some(creds) = auth.device_credentials.take() {
        let _ = api.logout(&creds.refresh_token);
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
            Ok(device_id)
        }
        Err(err) => Err(err),
    }
}

/// Logs out (best-effort server-side revoke), clears auth/upload state, and
/// disables screenshot capture.
pub fn logout<A: ApiTransport>(
    auth: &mut AuthState,
    screenshot: &mut ScreenshotState,
    upload: &mut UploadState,
    api: &A,
) {
    if let Some(creds) = auth.device_credentials.take() {
        let _ = api.logout(&creds.refresh_token);
    }
    upload.reset_for_logout();
    screenshot::disable(screenshot);
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

        let result = login(
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
        assert!(
            api.state().logout_calls.is_empty(),
            "first login has nothing to revoke"
        );
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
        let result = login(
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
        let result = login(
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
    fn logout_without_session_does_not_call_the_server() {
        let mut auth = AuthState::default();
        let mut screenshot = ScreenshotState::default();
        let mut upload = UploadState::default();
        let api = MockApiClient::new();
        logout(&mut auth, &mut screenshot, &mut upload, &api);
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
        logout(&mut auth, &mut screenshot, &mut upload, &api);
        assert_eq!(api.state().logout_calls.len(), 1);
        assert!(!screenshot.enabled);
        assert!(upload.device_credentials.is_none());
    }
}
