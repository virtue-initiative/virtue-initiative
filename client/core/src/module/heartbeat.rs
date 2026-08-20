use serde::{Deserialize, Serialize};

use crate::model::UploadKind;
use crate::module::upload::{self, UploadState};

pub(crate) const HEARTBEAT_INTERVAL_MS: i64 = 24 * 60 * 60 * 1_000;

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct HeartbeatState {
    pub last_heartbeat_ms: i64,
}

/// Phase 4: emit a heartbeat once per [`HEARTBEAT_INTERVAL_MS`] while
/// authenticated (authentication is read from `upload.device_credentials`,
/// the single source of truth — no separately-tracked flag needed here).
pub fn tick(state: &mut HeartbeatState, upload: &mut UploadState, now_ms: i64) {
    if upload.device_credentials.is_none() {
        return;
    }
    if now_ms - state.last_heartbeat_ms >= HEARTBEAT_INTERVAL_MS {
        state.last_heartbeat_ms = now_ms;
        upload::enqueue(upload, now_ms, 0.0, UploadKind::Heartbeat);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DeviceCredentials;

    const DAY_MS: i64 = HEARTBEAT_INTERVAL_MS + 1_000;

    #[allow(clippy::field_reassign_with_default)]
    fn authenticated_upload() -> UploadState {
        let mut upload = UploadState::default();
        upload.device_credentials = Some(DeviceCredentials {
            device_id: "d".into(),
            refresh_token: "r".into(),
        });
        upload
    }

    fn has_heartbeat(upload: &UploadState) -> bool {
        upload
            .pending_hash_events
            .iter()
            .any(|e| matches!(e.event, UploadKind::Heartbeat))
    }

    #[test]
    fn no_heartbeat_when_unauthenticated() {
        let mut state = HeartbeatState::default();
        let mut upload = UploadState::default();
        tick(&mut state, &mut upload, DAY_MS);
        assert!(!has_heartbeat(&upload));
    }

    #[test]
    fn first_tick_after_24h_from_epoch_emits_heartbeat() {
        let mut state = HeartbeatState::default();
        let mut upload = authenticated_upload();
        tick(&mut state, &mut upload, DAY_MS);
        assert!(has_heartbeat(&upload));
    }

    #[test]
    fn second_tick_within_24h_does_not_emit() {
        let mut state = HeartbeatState::default();
        let mut upload = authenticated_upload();
        tick(&mut state, &mut upload, DAY_MS);
        upload.pending_hash_events.clear();
        tick(&mut state, &mut upload, DAY_MS + 3_600_000);
        assert!(!has_heartbeat(&upload));
    }

    #[test]
    fn tick_after_another_24h_emits_again() {
        let mut state = HeartbeatState::default();
        let mut upload = authenticated_upload();
        tick(&mut state, &mut upload, DAY_MS);
        upload.pending_hash_events.clear();
        tick(&mut state, &mut upload, DAY_MS + HEARTBEAT_INTERVAL_MS);
        assert!(has_heartbeat(&upload));
    }

    #[test]
    fn heartbeat_upload_has_zero_risk() {
        let mut state = HeartbeatState::default();
        let mut upload = authenticated_upload();
        tick(&mut state, &mut upload, DAY_MS);
        let entry = upload
            .pending_hash_events
            .iter()
            .find(|e| matches!(e.event, UploadKind::Heartbeat))
            .unwrap();
        assert_eq!(entry.risk, Some(0.0));
    }
}
