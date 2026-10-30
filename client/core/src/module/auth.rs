use std::time::{Duration, Instant};

use crate::api::ApiTransport;
use crate::error::CoreResult;
use crate::model::AuthState;
use crate::module::errors::{self, ErrorState};
use crate::module::screenshot::{self, ScreenshotState};
use crate::module::upload::{self, UploadState};

/// How long into a login the flush may start new requests. With the 5s limit
/// on each API request, one more request plus the revoke still replies well
/// inside the daemon's 30s request timeout.
const FLUSH_BUDGET: Duration = Duration::from_secs(15);

/// Logs in, then tries once to deliver what the existing session queued and
/// revokes it (a login while another session is still active would otherwise
/// leave the old device row active on the server forever). On success, updates `auth`, enables screenshot capture,
/// and seeds `upload` with the fresh credentials/settings/hash token. Returns
/// the new device id. On failure, changes nothing (CORE-008).
#[allow(clippy::too_many_arguments)]
pub fn login<A: ApiTransport>(
    auth: &mut AuthState,
    screenshot: &mut ScreenshotState,
    upload: &mut UploadState,
    errors: &mut ErrorState,
    api: &A,
    device_name: &str,
    platform_name: &str,
    email: &str,
    password: &str,
    device_name_override: Option<&str>,
    now_ms: i64,
) -> CoreResult<String> {
    let started_at = Instant::now();
    let resolved_name = device_name_override
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(device_name);

    match api.register_device(email, password, resolved_name, platform_name) {
        Ok(registered) => {
            flush_queued_events(upload, errors, api, now_ms, started_at + FLUSH_BUDGET);
            if let Some(creds) = auth.device_credentials.take() {
                let _ = api.logout(&creds.refresh_token);
            }
            let device_id = registered.credentials.device_id.clone();
            auth.device_credentials = Some(registered.credentials.clone());
            // Kept so every platform's status page can name the signed-in
            // account without stashing the email separately (CORE-010).
            auth.account_email = Some(email.trim().to_string());
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

/// CORE-008: the last chance for events queued under a session about to be
/// revoked, since `reset_for_login` then clears them. Hash results are
/// committed first so that those events are eligible for the batch.
fn flush_queued_events<A: ApiTransport>(
    upload: &mut UploadState,
    errors: &mut ErrorState,
    api: &A,
    now_ms: i64,
    deadline: Instant,
) {
    upload::request_immediate_flush(upload, now_ms);
    if Instant::now() < deadline
        && let Some(plan) = upload::plan_hash_retries(upload, now_ms, true)
    {
        let outcome = upload::execute_hash_retries(plan.with_deadline(deadline), api);
        errors::record_all(errors, now_ms, outcome.errors.clone());
        upload::commit_hash_retries(upload, outcome, now_ms);
    }
    // The interval is ignored, because the flush requested above makes the batch urgent.
    if Instant::now() < deadline
        && let Some(plan) = upload::plan_batch(upload, now_ms, 0, true)
    {
        let outcome = upload::execute_batch(plan, api);
        if let Some((context, message)) = outcome.error() {
            errors::record(errors, now_ms, &context, message);
        }
        upload::commit_batch(upload, outcome, now_ms);
    }
    let unsent = upload.pending_hash_events.len() + upload.pending_batch_events.len();
    if unsent > 0 {
        errors::record(
            errors,
            now_ms,
            "login_flush",
            format!("discarded {unsent} queued events that could not be uploaded before login"),
        );
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
    auth.account_email = None;
    upload.reset_for_logout();
    screenshot::disable(screenshot);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MockApiClient;

    // CORE-008: login.
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
            &mut ErrorState::default(),
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

    // CORE-008: login.
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
            &mut ErrorState::default(),
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
            &mut ErrorState::default(),
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

    // CORE-008: login.
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
            &mut ErrorState::default(),
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
        let mut auth = AuthState::default();
        let mut screenshot = ScreenshotState::default();
        let mut upload = UploadState::default();
        let api = MockApiClient::new();
        api.program_register_device(Err(crate::error::CoreError::InvalidState("bad creds")));
        let result = login(
            &mut auth,
            &mut screenshot,
            &mut upload,
            &mut ErrorState::default(),
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

    // CORE-009: logout.
    #[test]
    fn logout_without_session_does_not_call_the_server() {
        let mut auth = AuthState::default();
        let mut screenshot = ScreenshotState::default();
        let mut upload = UploadState::default();
        let api = MockApiClient::new();
        logout(&mut auth, &mut screenshot, &mut upload, &api);
        assert!(api.state().logout_calls.is_empty());
    }

    // CORE-009: logout.
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
            &mut ErrorState::default(),
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
