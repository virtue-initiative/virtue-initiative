use serde::{Deserialize, Serialize};

use crate::api::{ApiTransport, DeviceCodePoll, DeviceCodeStart, RegisteredDevice};
use crate::error::{CoreError, CoreResult};
use crate::model::{AuthState, PendingCodeLogin};
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

    let resolved_name = resolve_device_name(device_name, device_name_override);

    let registered = api.register_device(email, password, resolved_name, platform_name)?;
    Ok(adopt_registration(
        auth, screenshot, upload, registered, now_ms,
    ))
}

/// The state transition CORE-008 specifies, shared by the password login above
/// and the pairing-code login below so the two cannot drift apart. Returns the
/// new device id.
fn adopt_registration(
    auth: &mut AuthState,
    screenshot: &mut ScreenshotState,
    upload: &mut UploadState,
    registered: RegisteredDevice,
    now_ms: i64,
) -> String {
    let device_id = registered.credentials.device_id.clone();
    auth.device_credentials = Some(registered.credentials.clone());
    // Whichever path got here, any half-finished pairing is now moot: a
    // password login that abandoned one must not leave it behind to be polled.
    auth.pending_code_login = None;
    // Kept so every platform's status page can name the signed-in
    // account without stashing the email separately (CORE-010).
    auth.account_email = Some(registered.account_email);
    upload.reset_for_login();
    upload.device_credentials = Some(registered.credentials);
    upload.settings = Some(registered.settings);
    upload.hash_token_cache = Some((registered.hash_token, now_ms));
    screenshot::enable(screenshot);
    device_id
}

/// Starts a passwordless sign-in (CORE-020). Revokes any existing session first,
/// for the same reason `login` does, then asks the API for a pairing and stashes
/// it in persisted state. Nothing about the client's logged-in state changes yet:
/// no device exists until the user approves the code and a poll collects it.
pub fn begin_code_login<A: ApiTransport>(
    auth: &mut AuthState,
    api: &A,
    device_name: &str,
    platform_name: &str,
    device_name_override: Option<&str>,
) -> CoreResult<DeviceCodeStart> {
    if let Some(creds) = auth.device_credentials.take() {
        let _ = api.logout(&creds.refresh_token);
    }

    let resolved_name = resolve_device_name(device_name, device_name_override).to_string();
    let start = api.start_device_code(&resolved_name, platform_name)?;

    auth.pending_code_login = Some(PendingCodeLogin {
        device_code: start.device_code.clone().into(),
        user_code: start.user_code.clone(),
        device_name: resolved_name,
        expires_at_ms: start.expires_at_ms,
        interval_seconds: start.interval_seconds,
    });

    Ok(start)
}

/// Asks whether the pending pairing has been approved (CORE-021). On approval
/// this lands in exactly the state a password login would have produced; on
/// expiry it clears the pairing and leaves the client logged out. A network
/// error leaves the pairing in place so the caller can simply poll again.
pub fn poll_code_login<A: ApiTransport>(
    auth: &mut AuthState,
    screenshot: &mut ScreenshotState,
    upload: &mut UploadState,
    api: &A,
    now_ms: i64,
) -> CoreResult<CodeLoginPoll> {
    let device_code = auth
        .pending_code_login
        .as_ref()
        .map(|pending| pending.device_code.0.clone())
        .ok_or(CoreError::InvalidState("no pairing code is pending"))?;

    match api.poll_device_code(&device_code)? {
        DeviceCodePoll::Pending => Ok(CodeLoginPoll::Pending),
        DeviceCodePoll::Expired => {
            auth.pending_code_login = None;
            Ok(CodeLoginPoll::Expired)
        }
        DeviceCodePoll::Approved(registered) => {
            auth.pending_code_login = None;
            let device_id = adopt_registration(auth, screenshot, upload, *registered, now_ms);
            Ok(CodeLoginPoll::Approved { device_id })
        }
    }
}

/// The outcome of one `poll_code_login` call (CORE-021). Serializable because it
/// is also the payload `ipc.rs` sends back to a separate client process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CodeLoginPoll {
    Pending,
    Approved { device_id: String },
    Expired,
}

/// CORE-008: a blank or absent override falls back to the platform's configured
/// default name.
fn resolve_device_name<'a>(device_name: &'a str, override_name: Option<&'a str>) -> &'a str {
    override_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(device_name)
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
    auth.account_email = None;
    auth.pending_code_login = None;
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
        assert_eq!(auth.account_email.as_deref(), Some("alice@example.org"));
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

    /// The fields CORE-008 promises a successful login leaves behind, so the
    /// password and pairing paths can be asserted against the same shape.
    fn login_shape(
        auth: &AuthState,
        screenshot: &ScreenshotState,
        upload: &UploadState,
    ) -> (Option<String>, Option<String>, Option<String>, bool, bool) {
        (
            auth.device_credentials
                .as_ref()
                .map(|c| c.device_id.clone()),
            auth.account_email.clone(),
            upload.hash_token_cache.as_ref().map(|(t, _)| t.clone()),
            upload.settings.is_some(),
            screenshot.enabled,
        )
    }

    #[test]
    fn begin_code_login_stores_the_pending_pairing_without_logging_in() {
        let mut auth = AuthState::default();
        let api = MockApiClient::new();

        let start = begin_code_login(&mut auth, &api, "test-device", "test-platform", None)
            .expect("begin should succeed");

        assert_eq!(start.user_code, "K7R-M3X");
        assert_eq!(start.interval_seconds, 5);
        let pending = auth.pending_code_login.as_ref().expect("pairing pending");
        assert_eq!(pending.user_code, "K7R-M3X");
        assert_eq!(pending.device_code.0, "dpc_mock");
        assert_eq!(pending.device_name, "test-device");
        assert_eq!(
            api.state().start_device_code_calls[0].platform,
            "test-platform"
        );
        // No device exists yet, so nothing about the logged-in state may change.
        assert!(auth.device_credentials.is_none());
        assert!(auth.account_email.is_none());
    }

    #[test]
    fn begin_code_login_uses_the_device_name_override() {
        let mut auth = AuthState::default();
        let api = MockApiClient::new();

        let _ = begin_code_login(
            &mut auth,
            &api,
            "test-device",
            "test-platform",
            Some("  My Laptop  "),
        );

        assert_eq!(api.state().start_device_code_calls[0].name, "My Laptop");
        assert_eq!(
            auth.pending_code_login
                .as_ref()
                .map(|p| p.device_name.clone()),
            Some("My Laptop".to_string())
        );
    }

    #[test]
    fn begin_code_login_revokes_an_existing_session_first() {
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
        let _ = begin_code_login(&mut auth, &api, "d", "p", None).expect("begin");

        assert_eq!(api.state().logout_calls.len(), 1);
        assert!(auth.device_credentials.is_none());
    }

    #[test]
    fn approved_poll_lands_in_the_same_state_a_password_login_produces() {
        let api = MockApiClient::new();

        let mut password_auth = AuthState::default();
        let mut password_screenshot = ScreenshotState::default();
        let mut password_upload = UploadState::default();
        let _ = login(
            &mut password_auth,
            &mut password_screenshot,
            &mut password_upload,
            &api,
            "d",
            "p",
            "mock@example.org",
            "secret",
            None,
            1_000,
        )
        .expect("password login");

        let mut code_auth = AuthState::default();
        let mut code_screenshot = ScreenshotState::default();
        let mut code_upload = UploadState::default();
        let _ = begin_code_login(&mut code_auth, &api, "d", "p", None).expect("begin");
        api.program_poll_device_code(Ok(api.approved_device_code()));

        let outcome = poll_code_login(
            &mut code_auth,
            &mut code_screenshot,
            &mut code_upload,
            &api,
            1_000,
        )
        .expect("poll");

        assert_eq!(
            outcome,
            CodeLoginPoll::Approved {
                device_id: "mock-device".to_string()
            }
        );
        assert_eq!(
            login_shape(&code_auth, &code_screenshot, &code_upload),
            login_shape(&password_auth, &password_screenshot, &password_upload)
        );
        assert!(code_auth.pending_code_login.is_none());
    }

    #[test]
    fn pending_poll_leaves_everything_untouched() {
        let mut auth = AuthState::default();
        let mut screenshot = ScreenshotState::default();
        let mut upload = UploadState::default();
        let api = MockApiClient::new();
        let _ = begin_code_login(&mut auth, &api, "d", "p", None).expect("begin");

        let outcome =
            poll_code_login(&mut auth, &mut screenshot, &mut upload, &api, 1_000).expect("poll");

        assert_eq!(outcome, CodeLoginPoll::Pending);
        assert!(auth.pending_code_login.is_some());
        assert!(auth.device_credentials.is_none());
        assert!(!screenshot.enabled);
        assert_eq!(api.state().poll_device_code_calls, vec!["dpc_mock"]);
    }

    #[test]
    fn a_password_login_clears_an_abandoned_pairing() {
        // The Linux CLI lets the user press Enter mid-pairing to sign in with a
        // password instead; the code they walked away from must not survive.
        let mut auth = AuthState::default();
        let mut screenshot = ScreenshotState::default();
        let mut upload = UploadState::default();
        let api = MockApiClient::new();
        let _ = begin_code_login(&mut auth, &api, "d", "p", None).expect("begin");
        assert!(auth.pending_code_login.is_some());

        login(
            &mut auth,
            &mut screenshot,
            &mut upload,
            &api,
            "d",
            "p",
            "user@example.com",
            "pw",
            None,
            1_000,
        )
        .expect("login");

        assert!(auth.pending_code_login.is_none());
        assert!(auth.device_credentials.is_some());
    }

    #[test]
    fn expired_poll_clears_the_pairing_and_leaves_the_client_logged_out() {
        let mut auth = AuthState::default();
        let mut screenshot = ScreenshotState::default();
        let mut upload = UploadState::default();
        let api = MockApiClient::new();
        let _ = begin_code_login(&mut auth, &api, "d", "p", None).expect("begin");
        api.program_poll_device_code(Ok(DeviceCodePoll::Expired));

        let outcome =
            poll_code_login(&mut auth, &mut screenshot, &mut upload, &api, 1_000).expect("poll");

        assert_eq!(outcome, CodeLoginPoll::Expired);
        assert!(auth.pending_code_login.is_none());
        assert!(auth.device_credentials.is_none());
        assert!(!screenshot.enabled);
    }

    #[test]
    fn a_failed_poll_keeps_the_pairing_so_the_caller_can_retry() {
        let mut auth = AuthState::default();
        let mut screenshot = ScreenshotState::default();
        let mut upload = UploadState::default();
        let api = MockApiClient::new();
        let _ = begin_code_login(&mut auth, &api, "d", "p", None).expect("begin");
        api.program_poll_device_code(Err(crate::error::CoreError::InvalidState("offline")));

        assert!(poll_code_login(&mut auth, &mut screenshot, &mut upload, &api, 1_000).is_err());
        assert!(auth.pending_code_login.is_some());
    }

    #[test]
    fn polling_with_nothing_pending_is_an_error() {
        let mut auth = AuthState::default();
        let mut screenshot = ScreenshotState::default();
        let mut upload = UploadState::default();
        let api = MockApiClient::new();

        assert!(poll_code_login(&mut auth, &mut screenshot, &mut upload, &api, 1_000).is_err());
        assert!(api.state().poll_device_code_calls.is_empty());
    }

    #[test]
    fn logout_clears_a_pending_pairing() {
        let mut auth = AuthState::default();
        let mut screenshot = ScreenshotState::default();
        let mut upload = UploadState::default();
        let api = MockApiClient::new();
        let _ = begin_code_login(&mut auth, &api, "d", "p", None).expect("begin");

        logout(&mut auth, &mut screenshot, &mut upload, &api);

        assert!(auth.pending_code_login.is_none());
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
        assert!(auth.account_email.is_none());
    }
}
