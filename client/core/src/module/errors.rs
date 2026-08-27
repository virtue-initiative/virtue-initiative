use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::model::StatusError;

/// How many recent errors are kept. CORE-018 requires the ring be capped so
/// persisted state can't grow without bound.
const MAX_RECENT_ERRORS: usize = 20;

/// Newest-first ring of the errors the daemon most recently hit, persisted as
/// part of `DaemonState` so it survives the crash-and-restart it exists to
/// explain (CORE-018).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ErrorState {
    pub recent: VecDeque<StatusError>,
}

/// Records one error, newest first, dropping the oldest past the cap.
pub fn record(state: &mut ErrorState, at_ms: i64, context: &str, message: impl Into<String>) {
    state.recent.push_front(StatusError {
        at_ms,
        context: context.to_string(),
        message: message.into(),
    });
    state.recent.truncate(MAX_RECENT_ERRORS);
}

/// Records a batch of `(context, message)` pairs collected by an `execute_*`
/// phase, which runs without access to state.
pub fn record_all(state: &mut ErrorState, at_ms: i64, errors: Vec<(String, String)>) {
    for (context, message) in errors {
        record(state, at_ms, &context, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_error_is_first() {
        let mut state = ErrorState::default();
        record(&mut state, 1, "batch_upload", "first");
        record(&mut state, 2, "hash_upload", "second");

        assert_eq!(state.recent.len(), 2);
        assert_eq!(state.recent[0].message, "second");
        assert_eq!(state.recent[0].context, "hash_upload");
        assert_eq!(state.recent[0].at_ms, 2);
        assert_eq!(state.recent[1].message, "first");
    }

    #[test]
    fn ring_is_capped_and_drops_the_oldest() {
        let mut state = ErrorState::default();
        for i in 0..(MAX_RECENT_ERRORS as i64 + 5) {
            record(&mut state, i, "batch_upload", format!("error {i}"));
        }

        assert_eq!(state.recent.len(), MAX_RECENT_ERRORS);
        assert_eq!(
            state.recent[0].message,
            format!("error {}", MAX_RECENT_ERRORS + 4)
        );
        assert_eq!(state.recent[MAX_RECENT_ERRORS - 1].message, "error 5");
    }

    #[test]
    fn record_all_preserves_order_newest_last_pushed() {
        let mut state = ErrorState::default();
        record_all(
            &mut state,
            7,
            vec![
                ("hash_upload".to_string(), "a".to_string()),
                ("batch_upload".to_string(), "b".to_string()),
            ],
        );

        assert_eq!(state.recent[0].message, "b");
        assert_eq!(state.recent[1].message, "a");
        assert!(state.recent.iter().all(|entry| entry.at_ms == 7));
    }
}
