use crate::config::Config;
use crate::daemon::DaemonState;
use crate::model::ServiceStatus;

/// Pure assembly of `ServiceStatus` from the daemon's persisted state plus the
/// compile-time `Config` (CORE-010). Works equally whether the daemon loop is
/// actually running: callers with no live daemon to ask (e.g. a CLI whose
/// resident process is stopped) can load a persisted `DaemonState` from disk
/// and pass it straight through with `is_running: false`, rather than
/// fabricating a status that's blind to real auth/pending-request state.
pub fn build(state: &DaemonState, config: &Config, is_running: bool) -> ServiceStatus {
    let settings = state.upload.settings.as_ref();
    ServiceStatus {
        is_authenticated: state.auth.device_credentials.is_some(),
        is_running,
        account_email: state.auth.account_email.clone(),
        device_id: state
            .auth
            .device_credentials
            .as_ref()
            .map(|c| c.device_id.clone()),
        device_name: settings.map(|s| s.name.clone()),
        // Wrapping keys are the owner's own key followed by every accepted
        // partner, so the partner count is one fewer. `None` until settings
        // have been fetched at all — that isn't the same as having 0 partners.
        partner_count: settings.map(|s| s.wrapping_keys.len().saturating_sub(1)),
        pending_hash_count: state.upload.pending_hash_events.len(),
        pending_batch_count: state.upload.pending_batch_events.len(),
        pending_request_count: state.upload.pending_request_count(),
        last_loop_at_ms: state.last_tick_at_ms,
        last_screenshot_attempt_at_ms: state.screenshot.last_attempt_at_ms,
        last_screenshot_at_ms: state.screenshot.last_capture_at_ms,
        last_skip_reason: state.screenshot.last_skip_reason.clone(),
        last_batch_at_ms: state.upload.last_batch_at_ms,
        recent_errors: state.errors.recent.iter().cloned().collect(),
        api_base_url: config.api_base_url.clone(),
        hash_base_url: settings.and_then(|s| s.hash_base_url.clone()),
        capture_interval_seconds: config.screenshot_interval.as_secs(),
        batch_window_seconds: config.batch_interval.as_secs(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::model::{BatchRecipient, DeviceCredentials, DeviceSettings, StatusSkipReason};
    use crate::module::errors;

    fn config() -> Config {
        Config::new(
            "https://api.example.org",
            "test-device",
            "test-platform",
            PathBuf::from("/tmp/virtue-status-test"),
            Duration::from_secs(300),
            Duration::from_secs(60),
        )
    }

    fn settings(partner_count: usize) -> DeviceSettings {
        DeviceSettings {
            device_id: "dev-1".into(),
            name: "Test Laptop".into(),
            platform: "linux".into(),
            // The owner's own key plus one per partner.
            wrapping_keys: (0..partner_count + 1)
                .map(|i| BatchRecipient {
                    user_id: format!("user-{i}"),
                    pub_key_base64: "key".into(),
                })
                .collect(),
            hash_base_url: Some("https://hash.example.org".into()),
        }
    }

    fn authenticated_state() -> DaemonState {
        let mut state = DaemonState::default();
        state.auth.device_credentials = Some(DeviceCredentials {
            device_id: "dev-1".into(),
            refresh_token: "r".into(),
        });
        state.auth.account_email = Some("alice@example.org".into());
        state
    }

    #[test]
    fn unauthenticated_status_reflects_defaults() {
        let status = build(&DaemonState::default(), &config(), true);
        assert!(!status.is_authenticated);
        assert!(status.is_running);
        assert_eq!(status.device_id, None);
        assert_eq!(status.account_email, None);
        assert_eq!(status.partner_count, None);
        assert_eq!(status.pending_request_count, 0);
        assert_eq!(status.pending_hash_count, 0);
        assert_eq!(status.pending_batch_count, 0);
        assert!(status.recent_errors.is_empty());
    }

    #[test]
    fn authenticated_status_reports_account_device_and_queue_depths() {
        let mut state = authenticated_state();
        state.upload.settings = Some(settings(2));
        state.last_tick_at_ms = Some(1_234);
        state.upload.last_batch_at_ms = Some(1_200);
        for _ in 0..3 {
            state
                .upload
                .pending_batch_events
                .push(crate::module::upload::PendingBatchEvent {
                    ts: 0,
                    risk: 0.0,
                    encoded: vec![1],
                    is_screenshot: false,
                    notify: None,
                });
        }

        let status = build(&state, &config(), true);
        assert!(status.is_authenticated);
        assert_eq!(status.device_id.as_deref(), Some("dev-1"));
        assert_eq!(status.account_email.as_deref(), Some("alice@example.org"));
        assert_eq!(status.device_name.as_deref(), Some("Test Laptop"));
        assert_eq!(status.partner_count, Some(2));
        assert_eq!(status.last_loop_at_ms, Some(1_234));
        assert_eq!(status.last_batch_at_ms, Some(1_200));
        assert_eq!(status.pending_batch_count, 3);
        assert_eq!(status.pending_hash_count, 0);
        // The coarse legacy figure counts a non-empty batch queue as one.
        assert_eq!(status.pending_request_count, 1);
    }

    #[test]
    fn a_solo_account_reports_zero_partners_not_none() {
        let mut state = authenticated_state();
        state.upload.settings = Some(settings(0));
        assert_eq!(build(&state, &config(), true).partner_count, Some(0));
    }

    #[test]
    fn capture_timings_and_skip_reason_are_surfaced() {
        let mut state = authenticated_state();
        state.screenshot.last_attempt_at_ms = Some(500);
        state.screenshot.last_capture_at_ms = Some(400);
        state.screenshot.last_skip_reason = Some(StatusSkipReason::LockedOrScreensaver);

        let status = build(&state, &config(), true);
        assert_eq!(status.last_screenshot_attempt_at_ms, Some(500));
        assert_eq!(status.last_screenshot_at_ms, Some(400));
        assert_eq!(
            status.last_skip_reason,
            Some(StatusSkipReason::LockedOrScreensaver)
        );
    }

    #[test]
    fn recent_errors_are_surfaced_newest_first() {
        let mut state = authenticated_state();
        errors::record(&mut state.errors, 1, "hash_upload", "older");
        errors::record(&mut state.errors, 2, "batch_upload", "newer");

        let status = build(&state, &config(), true);
        assert_eq!(status.recent_errors.len(), 2);
        assert_eq!(status.recent_errors[0].message, "newer");
        assert_eq!(status.recent_errors[1].message, "older");
    }

    #[test]
    fn advanced_fields_come_from_config_and_settings() {
        let mut state = authenticated_state();
        state.upload.settings = Some(settings(1));

        let status = build(&state, &config(), true);
        assert_eq!(status.api_base_url, "https://api.example.org");
        assert_eq!(
            status.hash_base_url.as_deref(),
            Some("https://hash.example.org")
        );
        assert_eq!(status.capture_interval_seconds, 300);
        assert_eq!(status.batch_window_seconds, 60);
    }

    #[test]
    fn status_reflects_a_caller_supplied_is_running_value() {
        // A caller with no live daemon to ask (e.g. a stopped CLI daemon
        // process) computes status from persisted state alone and must be
        // able to report `is_running: false` while still reflecting real
        // auth/pending-request state.
        let status = build(&authenticated_state(), &config(), false);
        assert!(status.is_authenticated);
        assert!(!status.is_running);
    }
}
