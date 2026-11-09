use crate::api::ApiTransport;
use crate::daemon::DaemonState;
use crate::error::CoreResult;
use crate::module::screenshot;
use crate::module::upload::{self, POST_LOGIN_PROOF_BATCH_COUNT, UploadState};

/// Logs in. The new device is registered first, so that a failed login
/// changes nothing (CORE-008). Then tries once to deliver the pending batch
/// and logs out of any existing session (a login while another session is
/// still active would otherwise leave the old device row active on the server
/// forever), and starts the new session from a fresh state. Returns the new
/// device id.
#[allow(clippy::too_many_arguments)]
pub fn login<A: ApiTransport>(
    state: &mut DaemonState,
    api: &A,
    device_name: &str,
    platform_name: &str,
    email: &str,
    password: &str,
    device_name_override: Option<&str>,
    now_ms: i64,
) -> CoreResult<String> {
    let resolved_name = device_name_override
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(device_name);

    let registered = api.register_device(email, password, resolved_name, platform_name)?;

    flush_pending_batch(&mut state.upload, api, now_ms);
    logout(state, api);

    let device_id = registered.credentials.device_id.clone();
    state.auth.device_credentials = Some(registered.credentials.clone());
    // Kept so every platform's status page can name the signed-in
    // account without stashing the email separately (CORE-010).
    state.auth.account_email = Some(email.trim().to_string());
    state.upload.post_login_proof_batches_remaining = POST_LOGIN_PROOF_BATCH_COUNT;
    state.upload.device_credentials = Some(registered.credentials);
    state.upload.settings = Some(registered.settings);
    state.upload.hash_token_cache = Some((registered.hash_token, now_ms));
    screenshot::enable(&mut state.screenshot);
    Ok(device_id)
}

/// One attempt at the batch queued under a session about to end. Its outcome
/// is not committed, because the logout that follows resets the upload state.
fn flush_pending_batch<A: ApiTransport>(upload: &mut UploadState, api: &A, now_ms: i64) {
    upload::request_immediate_flush(upload, now_ms);
    // The interval is ignored, because the flush requested above makes the batch urgent.
    if let Some(plan) = upload::plan_batch(upload, now_ms, 0, true) {
        upload::execute_batch(plan, api);
    }
}

/// Logs out (best-effort server-side revoke) and resets the whole state, which
/// also disables screenshot capture (CORE-009).
pub fn logout<A: ApiTransport>(state: &mut DaemonState, api: &A) {
    if let Some(creds) = &state.auth.device_credentials {
        let _ = api.logout(&creds.refresh_token);
    }
    *state = DaemonState::default();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MockApiClient;

    fn login_as(state: &mut DaemonState, api: &MockApiClient, email: &str) -> CoreResult<String> {
        login(state, api, "d", "p", email, "secret", None, 1_000)
    }

    // CORE-008: login.
    #[test]
    fn login_success_sets_state_and_enables_screenshots() {
        let mut state = DaemonState::default();
        let api = MockApiClient::new();

        let result = login_as(&mut state, &api, "alice@example.org");
        assert!(result.is_ok());
        assert!(
            api.state().logout_calls.is_empty(),
            "first login has nothing to revoke"
        );
        assert!(state.auth.device_credentials.is_some());
        assert!(state.upload.device_credentials.is_some());
        assert!(state.upload.settings.is_some());
        assert_eq!(
            state.auth.account_email.as_deref(),
            Some("alice@example.org")
        );
        assert!(state.screenshot.enabled);
    }

    // CORE-008: login.
    #[test]
    fn login_while_authenticated_revokes_previous_session() {
        let mut state = DaemonState::default();
        let api = MockApiClient::new();

        let _ = login_as(&mut state, &api, "alice@example.org");
        let result = login_as(&mut state, &api, "bob@example.org");
        assert!(result.is_ok());
        assert_eq!(api.state().logout_calls.len(), 1);
    }

    // CORE-008: login.
    #[test]
    fn login_uses_device_name_override_when_present() {
        let mut state = DaemonState::default();
        let api = MockApiClient::new();
        let _ = login(
            &mut state,
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

    // CORE-008: login.
    #[test]
    fn login_failure_leaves_state_logged_out() {
        let mut state = DaemonState::default();
        let api = MockApiClient::new();
        api.program_register_device(Err(crate::error::CoreError::InvalidState("bad creds")));
        let result = login_as(&mut state, &api, "alice@example.org");
        assert!(result.is_err());
        assert!(state.auth.device_credentials.is_none());
        assert!(!state.screenshot.enabled);
    }

    // CORE-009: logout.
    #[test]
    fn logout_without_session_does_not_call_the_server() {
        let mut state = DaemonState::default();
        let api = MockApiClient::new();
        logout(&mut state, &api);
        assert!(api.state().logout_calls.is_empty());
    }

    // CORE-009: logout.
    #[test]
    fn logout_with_session_revokes_and_disables_screenshots() {
        let mut state = DaemonState::default();
        let api = MockApiClient::new();
        let _ = login_as(&mut state, &api, "alice@example.org");
        logout(&mut state, &api);
        assert_eq!(api.state().logout_calls.len(), 1);
        assert!(!state.screenshot.enabled);
        assert!(state.upload.device_credentials.is_none());
        assert!(state.auth.account_email.is_none());
    }
}
