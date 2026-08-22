use crate::model::{AuthState, ServiceStatus};
use crate::module::upload::UploadState;

/// Pure assembly of `ServiceStatus` from `&AuthState`/`&UploadState`. Works
/// equally whether the daemon loop is actually running: callers with no live
/// daemon to ask (e.g. a CLI whose resident process is stopped) can load a
/// persisted `DaemonState` from disk and pass its `auth`/`upload`/
/// `last_tick_at_ms` straight through with `is_running: false`, rather than
/// fabricating a status that's blind to real auth/pending-request state.
pub fn build(
    auth: &AuthState,
    upload: &UploadState,
    last_loop_at_ms: Option<i64>,
    is_running: bool,
) -> ServiceStatus {
    ServiceStatus {
        is_authenticated: auth.device_credentials.is_some(),
        is_running,
        device_id: auth
            .device_credentials
            .as_ref()
            .map(|c| c.device_id.clone()),
        last_loop_at_ms,
        pending_request_count: upload.pending_request_count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DeviceCredentials;

    #[test]
    fn unauthenticated_status_reflects_defaults() {
        let status = build(&AuthState::default(), &UploadState::default(), None, true);
        assert!(!status.is_authenticated);
        assert!(status.is_running);
        assert_eq!(status.device_id, None);
        assert_eq!(status.pending_request_count, 0);
    }

    #[test]
    fn authenticated_status_reports_device_id_and_pending_count() {
        let auth = AuthState {
            device_credentials: Some(DeviceCredentials {
                device_id: "dev-1".into(),
                refresh_token: "r".into(),
            }),
        };
        let mut upload = UploadState::default();
        upload
            .pending_batch_events
            .push(crate::module::upload::PendingBatchEvent {
                ts: 0,
                risk: 0.0,
                encoded: vec![1],
                is_screenshot: false,
                notify: None,
            });
        let status = build(&auth, &upload, Some(1_234), true);
        assert!(status.is_authenticated);
        assert_eq!(status.device_id.as_deref(), Some("dev-1"));
        assert_eq!(status.last_loop_at_ms, Some(1_234));
        assert_eq!(status.pending_request_count, 1);
    }

    #[test]
    fn status_reflects_a_caller_supplied_is_running_value() {
        // A caller with no live daemon to ask (e.g. a stopped CLI daemon
        // process) computes status from persisted state alone and must be
        // able to report `is_running: false` while still reflecting real
        // auth/pending-request state.
        let auth = AuthState {
            device_credentials: Some(DeviceCredentials {
                device_id: "dev-1".into(),
                refresh_token: "r".into(),
            }),
        };
        let status = build(&auth, &UploadState::default(), None, false);
        assert!(status.is_authenticated);
        assert!(!status.is_running);
    }
}
