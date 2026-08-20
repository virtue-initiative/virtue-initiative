use serde::{Deserialize, Serialize};

use crate::model::UploadKind;
use crate::module::upload::{self, UploadState};

const FAILURE_WINDOW_MS: i64 = 30 * 60 * 1_000;
const FAILURE_THRESHOLD: usize = 5;

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct CaptureAvailabilityState {
    pub recent_failures_ms: Vec<i64>,
}

/// Records a screenshot capture failure. Called by `screenshot::commit`.
pub fn note_failure(state: &mut CaptureAvailabilityState, now_ms: i64) {
    state.recent_failures_ms.push(now_ms);
}

/// Phase 3: prune the failure window and alert (then reset) once the
/// threshold is crossed.
pub fn tick(state: &mut CaptureAvailabilityState, upload: &mut UploadState, now_ms: i64) {
    state
        .recent_failures_ms
        .retain(|&t| now_ms - t <= FAILURE_WINDOW_MS);
    if state.recent_failures_ms.len() >= FAILURE_THRESHOLD {
        upload::enqueue(upload, now_ms, 0.5, UploadKind::CaptureFailed);
        state.recent_failures_ms.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DeviceCredentials;

    #[allow(clippy::field_reassign_with_default)]
    fn authenticated_upload() -> UploadState {
        let mut upload = UploadState::default();
        upload.device_credentials = Some(DeviceCredentials {
            device_id: "d".into(),
            refresh_token: "r".into(),
        });
        upload
    }

    #[test]
    fn four_failures_below_threshold_no_upload() {
        let mut state = CaptureAvailabilityState::default();
        let mut upload = authenticated_upload();
        for _ in 0..4 {
            note_failure(&mut state, 1_000);
        }
        tick(&mut state, &mut upload, 1_000);
        assert!(upload.pending_hash_events.is_empty());
    }

    #[test]
    fn fifth_failure_triggers_capture_failed_upload() {
        let mut state = CaptureAvailabilityState::default();
        let mut upload = authenticated_upload();
        for _ in 0..5 {
            note_failure(&mut state, 1_000);
        }
        tick(&mut state, &mut upload, 1_000);
        assert!(
            upload
                .pending_hash_events
                .iter()
                .any(|e| matches!(e.event, UploadKind::CaptureFailed))
        );
        assert!(
            state.recent_failures_ms.is_empty(),
            "should reset after alerting"
        );
    }

    #[test]
    fn old_failures_age_out_of_the_window() {
        let mut state = CaptureAvailabilityState::default();
        let mut upload = authenticated_upload();
        for _ in 0..4 {
            note_failure(&mut state, 0);
        }
        // Past the 30-minute window: the old failures should be pruned before
        // a 5th (fresh) failure is evaluated.
        note_failure(&mut state, FAILURE_WINDOW_MS + 60_000);
        tick(&mut state, &mut upload, FAILURE_WINDOW_MS + 60_000);
        assert!(upload.pending_hash_events.is_empty());
    }
}
