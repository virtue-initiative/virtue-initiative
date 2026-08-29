//! Turning a forced ("Test Screenshot") capture into an honest confirmation.
//!
//! `Daemon::force_capture_now` returns as soon as the capture has been
//! committed and an immediate flush requested — the upload itself happens on a
//! later tick, over the network. A UI that says "uploaded" the moment that
//! call returns is guessing. This module polls `status()` afterwards and
//! reports what actually happened, so every platform tells the same story from
//! the same signals (`last_screenshot_at_ms`, `last_batch_at_ms` and the
//! pending queues — CORE-010).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::model::{ServiceStatus, StatusSkipReason};

/// How long to wait for the forced capture's batch to land before telling the
/// user it is still in flight.
pub const DEFAULT_UPLOAD_TIMEOUT: Duration = Duration::from_secs(20);

/// How often to re-read `status()` while waiting.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// What a forced capture actually did, once its batch has had a chance to
/// reach the server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", content = "reason", rename_all = "snake_case")]
pub enum ForcedCaptureOutcome {
    /// A screenshot was captured and a batch landed with every queued event.
    Uploaded,
    /// No screenshot was taken: one of the capture gates rejected it.
    NotCaptured(Option<StatusSkipReason>),
    /// A screenshot was captured, but no batch had landed by the deadline.
    /// The daemon keeps retrying on its own.
    Pending,
}

impl ForcedCaptureOutcome {
    /// Stable identifier for UIs that pick their own (localized) wording —
    /// Android's `strings.xml`, for instance.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Uploaded => "uploaded",
            Self::NotCaptured(_) => "not_captured",
            Self::Pending => "pending",
        }
    }

    /// The `{"outcome": …, "message": …}` payload a platform FFI hands its UI.
    pub fn report_json(&self) -> String {
        serde_json::json!({ "outcome": self.code(), "message": self.message() }).to_string()
    }

    /// The message a client shows the user. Shared so the wording of the
    /// "Test Screenshot" confirmation is identical on every platform.
    pub fn message(&self) -> &'static str {
        match self {
            Self::Uploaded => "Screenshot uploaded. Check the web logs page to view it.",
            Self::NotCaptured(Some(StatusSkipReason::StaticScreen)) => {
                "No screenshot was taken: the screen hasn't changed since the last one."
            }
            Self::NotCaptured(Some(StatusSkipReason::LockedOrScreensaver)) => {
                "No screenshot was taken: the screen is locked or the screensaver is on."
            }
            Self::NotCaptured(Some(StatusSkipReason::CaptureFailed)) => {
                "The screen could not be captured. See the status page for details."
            }
            Self::NotCaptured(None) => "No screenshot was taken.",
            Self::Pending => {
                "Screenshot captured, but it hasn't finished uploading yet. \
                 The client will keep trying in the background."
            }
        }
    }
}

/// Polls `status` until the forced capture's work has left both queues and a
/// batch has landed, or `timeout` elapses.
///
/// `before` MUST be a status read *before* `force_capture_now` was called:
/// `last_screenshot_at_ms` distinguishes a real capture from a gated one, and
/// `last_batch_at_ms` is the baseline a new batch has to move past.
///
/// `sleep` is injected so tests don't actually wait; production callers pass
/// `std::thread::sleep`.
pub fn wait_for_upload(
    before: &ServiceStatus,
    timeout: Duration,
    poll_interval: Duration,
    mut status: impl FnMut() -> CoreResult<ServiceStatus>,
    mut sleep: impl FnMut(Duration),
) -> CoreResult<ForcedCaptureOutcome> {
    let mut current = status()?;

    // The capture itself is synchronous inside `force_capture_now`, so this
    // first read already knows whether a screenshot was taken.
    if current.last_screenshot_at_ms == before.last_screenshot_at_ms {
        return Ok(ForcedCaptureOutcome::NotCaptured(current.last_skip_reason));
    }

    let mut waited = Duration::ZERO;
    loop {
        if uploaded(before, &current) {
            return Ok(ForcedCaptureOutcome::Uploaded);
        }
        if waited >= timeout {
            return Ok(ForcedCaptureOutcome::Pending);
        }
        sleep(poll_interval);
        waited += poll_interval;
        current = status()?;
    }
}

/// A batch landed after the baseline *and* nothing is still queued — so the
/// event this capture produced is among what went up, not waiting behind it.
fn uploaded(before: &ServiceStatus, current: &ServiceStatus) -> bool {
    current.last_batch_at_ms.is_some()
        && current.last_batch_at_ms != before.last_batch_at_ms
        && current.pending_hash_count == 0
        && current.pending_batch_count == 0
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn status_at(screenshot_ms: Option<i64>, batch_ms: Option<i64>) -> ServiceStatus {
        ServiceStatus {
            last_screenshot_at_ms: screenshot_ms,
            last_batch_at_ms: batch_ms,
            ..ServiceStatus::default()
        }
    }

    /// Feeds the given statuses one per poll, and records how long the caller
    /// asked to sleep in total.
    fn run(
        before: &ServiceStatus,
        polls: Vec<ServiceStatus>,
        timeout: Duration,
    ) -> (ForcedCaptureOutcome, Duration) {
        let remaining = RefCell::new(polls.into_iter());
        let slept = RefCell::new(Duration::ZERO);
        let outcome = wait_for_upload(
            before,
            timeout,
            Duration::from_millis(500),
            || {
                Ok(remaining
                    .borrow_mut()
                    .next()
                    .expect("polled more times than the test expected"))
            },
            |d| *slept.borrow_mut() += d,
        )
        .expect("wait");
        (outcome, slept.into_inner())
    }

    #[test]
    fn reports_uploaded_once_a_new_batch_lands_with_empty_queues() {
        let before = status_at(Some(1_000), Some(2_000));
        let mut mid = status_at(Some(5_000), Some(2_000));
        mid.pending_batch_count = 1;
        let (outcome, _) = run(
            &before,
            vec![mid, status_at(Some(5_000), Some(6_000))],
            DEFAULT_UPLOAD_TIMEOUT,
        );
        assert_eq!(outcome, ForcedCaptureOutcome::Uploaded);
    }

    #[test]
    fn a_first_ever_batch_counts_as_an_upload() {
        let before = status_at(Some(1_000), None);
        let (outcome, _) = run(
            &before,
            vec![status_at(Some(5_000), Some(6_000))],
            DEFAULT_UPLOAD_TIMEOUT,
        );
        assert_eq!(outcome, ForcedCaptureOutcome::Uploaded);
    }

    #[test]
    fn a_batch_that_left_events_behind_is_not_an_upload_yet() {
        let before = status_at(Some(1_000), Some(2_000));
        let mut landed_but_queued = status_at(Some(5_000), Some(6_000));
        landed_but_queued.pending_hash_count = 1;
        let (outcome, _) = run(&before, vec![landed_but_queued], Duration::from_millis(0));
        assert_eq!(outcome, ForcedCaptureOutcome::Pending);
    }

    #[test]
    fn reports_the_skip_reason_when_no_screenshot_was_taken() {
        let before = status_at(Some(1_000), Some(2_000));
        let mut skipped = status_at(Some(1_000), Some(2_000));
        skipped.last_skip_reason = Some(StatusSkipReason::LockedOrScreensaver);
        let (outcome, slept) = run(&before, vec![skipped], DEFAULT_UPLOAD_TIMEOUT);
        assert_eq!(
            outcome,
            ForcedCaptureOutcome::NotCaptured(Some(StatusSkipReason::LockedOrScreensaver))
        );
        // A gated capture is known immediately — nothing to wait for.
        assert_eq!(slept, Duration::ZERO);
    }

    #[test]
    fn gives_up_after_the_timeout_and_reports_the_upload_as_still_in_flight() {
        let before = status_at(Some(1_000), Some(2_000));
        let stuck = status_at(Some(5_000), Some(2_000));
        let (outcome, slept) = run(
            &before,
            vec![stuck.clone(), stuck.clone(), stuck.clone()],
            Duration::from_secs(1),
        );
        assert_eq!(outcome, ForcedCaptureOutcome::Pending);
        assert_eq!(slept, Duration::from_secs(1));
    }
}
