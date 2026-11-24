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
    server: Option<&str>,
    app_transaction_id: Option<&str>,
    now_ms: i64,
) -> CoreResult<String> {
    let resolved_name = device_name_override
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(device_name);

    // CORE-028: a chosen server is checked before anything is sent to it.
    let server = server
        .map(|address| api.resolve_server(address))
        .transpose()?;
    let api_base_url = server.as_ref().map(|s| s.api_base_url.as_str());
    let mut registered = api.register_device(
        api_base_url,
        email,
        password,
        resolved_name,
        platform_name,
        app_transaction_id,
    )?;
    registered.credentials.server = server;

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
        let _ = api.logout(creds.api_base_url(), &creds.refresh_token);
    }
    *state = DaemonState::initial();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MockApiClient;

    fn login_as(state: &mut DaemonState, api: &MockApiClient, email: &str) -> CoreResult<String> {
        login(
            state, api, "d", "p", email, "secret", None, None, None, 1_000,
        )
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
            None,
            None,
            1_000,
        );
        assert_eq!(api.state().register_device_calls[0].name, "My Laptop");
    }

    // CORE-008: login.
    #[test]
    fn login_registers_app_transaction_id_when_present() {
        let mut state = DaemonState::default();
        let api = MockApiClient::new();
        let _ = login(
            &mut state,
            &api,
            "d",
            "p",
            "alice@example.org",
            "secret",
            None,
            None,
            Some("2000000123456789"),
            1_000,
        );
        assert_eq!(
            api.state().register_device_calls[0]
                .app_transaction_id
                .as_deref(),
            Some("2000000123456789")
        );
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

    // CORE-008, CORE-028: a chosen server is resolved, used to register and
    // kept with the session.
    #[test]
    fn login_to_a_chosen_server_registers_there_and_keeps_it() {
        let mut state = DaemonState::default();
        let api = MockApiClient::new();
        let server = crate::model::ChosenServer {
            api_base_url: "https://virtue.example.org/api".into(),
            web_app_url: Some("https://virtue.example.org".into()),
        };
        api.program_resolve_server(Ok(server.clone()));
        login(
            &mut state,
            &api,
            "d",
            "p",
            "alice@example.org",
            "secret",
            None,
            Some("virtue.example.org"),
            None,
            1_000,
        )
        .unwrap();
        assert_eq!(api.state().resolve_server_calls, vec!["virtue.example.org"]);
        assert_eq!(
            api.state().api_base_urls,
            vec![("register_device", Some(server.api_base_url.clone()))]
        );
        let creds = state.upload.device_credentials.as_ref().unwrap();
        assert_eq!(creds.server.as_ref(), Some(&server));
        assert_eq!(
            state
                .auth
                .device_credentials
                .as_ref()
                .unwrap()
                .server
                .as_ref(),
            Some(&server)
        );
    }

    // CORE-008, CORE-028: a server that does not resolve fails the login and
    // nothing is sent anywhere.
    #[test]
    fn login_to_a_server_that_does_not_resolve_changes_nothing() {
        let mut state = DaemonState::default();
        let api = MockApiClient::new();
        let _ = login_as(&mut state, &api, "alice@example.org");
        api.program_resolve_server(Err(crate::error::CoreError::Remote(
            "No Virtue server found at https://nowhere.example.org.".into(),
        )));
        let result = login(
            &mut state,
            &api,
            "d",
            "p",
            "bob@example.org",
            "secret",
            None,
            Some("nowhere.example.org"),
            None,
            2_000,
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "No Virtue server found at https://nowhere.example.org."
        );
        assert_eq!(api.state().register_device_calls.len(), 1);
        assert!(api.state().logout_calls.is_empty());
        assert_eq!(
            state.auth.account_email.as_deref(),
            Some("alice@example.org")
        );
    }

    // CORE-009, CORE-028: logout revokes on the session's own server and
    // returns the device to the default server.
    #[test]
    fn logout_revokes_on_the_chosen_server_and_forgets_it() {
        let mut state = DaemonState::default();
        let api = MockApiClient::new();
        login(
            &mut state,
            &api,
            "d",
            "p",
            "alice@example.org",
            "secret",
            None,
            Some("https://virtue.example.org"),
            None,
            1_000,
        )
        .unwrap();
        logout(&mut state, &api);
        assert_eq!(
            api.state().api_base_urls.last(),
            Some(&("logout", Some("https://virtue.example.org".to_string())))
        );
        assert!(state.auth.device_credentials.is_none());
        assert!(state.upload.device_credentials.is_none());
    }
}
