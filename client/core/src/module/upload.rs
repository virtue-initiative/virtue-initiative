mod batch;

use serde::{Deserialize, Serialize};

use batch::BatchBuilder;
pub(crate) use batch::MAX_BATCH_ITEMS_PER_UPLOAD;

use crate::api::{ApiTransport, UploadedBatchResponse};
use crate::crypto::{CryptoEngine, compute_event_hash, encode_batch_event};
use crate::error::CoreError;
use crate::logging::{log_error, log_warning};
use crate::model::{
    BatchRecipient, BatchUpload, DeviceCredentials, DeviceSettings, LogEntry, NotifyPayload,
    UploadKind,
};

/// Wire message for `ClientController::queue_upload` / `Daemon::queue_upload` —
/// still needed for IPC serialization even though nothing dispatches it
/// through an event bus anymore.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Upload {
    pub risk: f32,
    pub kind: UploadKind,
}

/// Wire message for `ClientController::flush_batch_now` / `Daemon::flush_batch_now`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlushBatchNow;

pub(crate) const POST_LOGIN_PROOF_BATCH_COUNT: u32 = 3;
const MAX_HASH_RETRIES_PER_LOOP: usize = 8;
const HASH_TOKEN_MAX_AGE_MS: i64 = 55 * 60 * 1_000;

const INITIAL_BACKOFF_MS: i64 = 1_000; // 1s
const MAX_BACKOFF_MS: i64 = 20 * 60 * 1_000; // cap at 20 minutes

/// Risk-rating band thresholds mirroring `shared-web/risk.ts` (`getRiskRating`).
const HIGH_RISK_RATING: f32 = 0.7;
const MEDIUM_RISK_RATING: f32 = 0.4;

/// A hash-uploaded event awaiting batch upload, paired with its risk so the batch
/// upload can report how many high/medium-risk events it carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingBatchEvent {
    pub ts: i64,
    pub risk: f32,
    pub encoded: Vec<u8>,
    /// Whether the source event was `UploadKind::Screenshot`, so the batch upload
    /// can report `event_counts.screenshot` without decoding `encoded`.
    #[serde(default)]
    pub is_screenshot: bool,
    /// Alert-email metadata, set when the event's risk is >= `EXTRA_HIGH_RISK` at
    /// hash time. Rides with this event into the batch it's uploaded in — never
    /// sent standalone.
    #[serde(default)]
    pub notify: Option<NotifyPayload>,
}

/// Builds the notification payload for a high-risk event.
fn build_notify_payload(entry: &LogEntry) -> NotifyPayload {
    let value = serde_json::to_value(&entry.event).unwrap_or(serde_json::Value::Null);
    let event_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("event")
        .to_string();
    let data = value.get("data");
    let title = data
        .and_then(|d| d.get("title"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let details = data
        .and_then(|d| d.get("details"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    NotifyPayload {
        ts: entry.ts,
        event_type,
        risk: entry.risk.unwrap_or(0.0),
        title,
        details,
    }
}

/// Persisted per-queue exponential backoff (see `Daemon::new`'s comment on
/// why this now survives a restart, unlike the pre-rewrite in-memory version).
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct RetryBackoff {
    pub next_attempt_at_ms: i64,
    pub current_backoff_ms: i64, // 0 until first failure; treated as INITIAL_BACKOFF_MS on first use
}

impl RetryBackoff {
    fn ready(&self, now_ms: i64) -> bool {
        now_ms >= self.next_attempt_at_ms
    }

    fn record_failure(&mut self, now_ms: i64) {
        let backoff = if self.current_backoff_ms == 0 {
            INITIAL_BACKOFF_MS
        } else {
            self.current_backoff_ms
        };
        // Cheap jitter (not security-sensitive) so many devices hitting the same
        // outage don't all retry in lockstep once it clears.
        let jitter_ms = now_ms % 250;
        self.next_attempt_at_ms = now_ms + backoff + jitter_ms;
        self.current_backoff_ms = (backoff * 2).min(MAX_BACKOFF_MS);
    }

    fn record_success(&mut self) {
        self.next_attempt_at_ms = 0;
        self.current_backoff_ms = 0;
    }

    /// Makes the next attempt happen immediately, without resetting the
    /// growing `current_backoff_ms` — used by an explicit flush request
    /// (`Daemon::flush_batch_now`, or the shutdown flush) to bypass the
    /// cooldown for one attempt.
    fn make_ready_now(&mut self, now_ms: i64) {
        self.next_attempt_at_ms = now_ms;
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct UploadState {
    pub pending_batch_events: Vec<PendingBatchEvent>,
    pub pending_hash_events: Vec<LogEntry>,
    pub last_batch_at_ms: Option<i64>,
    pub post_login_proof_batches_remaining: u32,
    pub settings: Option<DeviceSettings>,
    pub device_credentials: Option<DeviceCredentials>,
    /// The last `seq` accepted by the hash server for this device — hash-server/SPEC.md
    /// §2.1 requires each `POST /hash` to carry a `seq` strictly greater than this.
    /// Reset to 0 in lockstep with every server-side reset: login, logout, and each
    /// batch upload that lands (the API resets the server's hash state right after
    /// storing the batch).
    pub next_hash_seq: u32,
    pub hash_backoff: RetryBackoff,
    pub batch_backoff: RetryBackoff,
    /// Cached hash-server JWT and the UTC-ms time it was fetched. Refreshed via
    /// `GET /d/device` once older than `HASH_TOKEN_MAX_AGE_MS`.
    pub hash_token_cache: Option<(String, i64)>,
    /// Set when a heartbeat or extra-high-risk event is enqueued (or an
    /// explicit flush is requested); makes the next batch plan bypass the
    /// interval-wait check. Cleared once a batch successfully lands.
    pub force_flush: bool,
    /// Set specifically by a heartbeat enqueue (or an explicit flush);
    /// additionally bypasses the screen-lock battery gate. Cleared once a
    /// batch successfully lands.
    pub bypass_lock: bool,
}

impl UploadState {
    pub fn reset_for_login(&mut self) {
        self.pending_batch_events.clear();
        self.pending_hash_events.clear();
        self.last_batch_at_ms = None;
        self.post_login_proof_batches_remaining = POST_LOGIN_PROOF_BATCH_COUNT;
        self.next_hash_seq = 0;
        self.force_flush = false;
        self.bypass_lock = false;
    }

    pub fn reset_for_logout(&mut self) {
        self.pending_batch_events.clear();
        self.pending_hash_events.clear();
        self.last_batch_at_ms = None;
        self.post_login_proof_batches_remaining = 0;
        self.settings = None;
        self.device_credentials = None;
        self.next_hash_seq = 0;
        self.hash_token_cache = None;
        self.force_flush = false;
        self.bypass_lock = false;
    }

    pub fn pending_request_count(&self) -> usize {
        self.pending_hash_events.len() + usize::from(!self.pending_batch_events.is_empty())
    }
}

/// Enqueues `kind` for hash + batch upload. Called by every other module
/// instead of publishing an `Upload` event. A no-op while unauthenticated.
pub fn enqueue(state: &mut UploadState, now_ms: i64, risk: f32, kind: UploadKind) {
    if state.device_credentials.is_none() {
        return;
    }
    let is_heartbeat = matches!(kind, UploadKind::Heartbeat);
    let is_high_risk = risk >= crate::module::lifecycle::EXTRA_HIGH_RISK;
    state.pending_hash_events.push(LogEntry {
        ts: now_ms,
        risk: Some(risk),
        event: kind,
    });
    if is_heartbeat {
        state.force_flush = true;
        state.bypass_lock = true;
    } else if is_high_risk {
        state.force_flush = true;
    }
}

/// Bypasses the batch/hash backoff cooldown and forces the next tick to flush
/// immediately, regardless of screen-lock state. Used by `Daemon::flush_batch_now`
/// and the daemon's shutdown-time flush.
pub fn request_immediate_flush(state: &mut UploadState, now_ms: i64) {
    state.force_flush = true;
    state.bypass_lock = true;
    state.hash_backoff.make_ready_now(now_ms);
    state.batch_backoff.make_ready_now(now_ms);
}

fn can_capture(settings: &DeviceSettings) -> bool {
    !settings.wrapping_keys.is_empty()
}

fn batch_recipients(settings: &DeviceSettings) -> Result<Vec<BatchRecipient>, CoreError> {
    if settings.wrapping_keys.is_empty() {
        return Err(CoreError::InvalidState("no batch recipients available"));
    }
    Ok(settings.wrapping_keys.clone())
}

// ── Hash retries: plan / execute / commit ──────────────────────────────────

pub struct HashRetryPlan {
    device_id: String,
    refresh_token: String,
    hash_base_url: Option<String>,
    cached_token: Option<(String, i64)>,
    events: Vec<LogEntry>,
    next_hash_seq: u32,
    now_ms: i64,
}

/// Phase 4: decide whether hash retries should run this tick, and snapshot
/// everything `execute_hash_retries` needs to make its network calls.
pub fn plan_hash_retries(
    state: &mut UploadState,
    now_ms: i64,
    screen_active: bool,
) -> Option<HashRetryPlan> {
    if state.pending_hash_events.is_empty() {
        return None;
    }
    if !(screen_active || state.bypass_lock) {
        return None;
    }
    if !state.hash_backoff.ready(now_ms) {
        return None;
    }
    let creds = state.device_credentials.clone()?;
    Some(HashRetryPlan {
        device_id: creds.device_id,
        refresh_token: creds.refresh_token,
        hash_base_url: state
            .settings
            .as_ref()
            .and_then(|s| s.hash_base_url.clone()),
        cached_token: state.hash_token_cache.clone(),
        // Drain rather than clone: cheaper, and commit_hash_retries splices
        // whatever's left back in.
        events: std::mem::take(&mut state.pending_hash_events),
        next_hash_seq: state.next_hash_seq,
        now_ms,
    })
}

pub struct HashRetryOutcome {
    device_id: String,
    still_pending: Vec<LogEntry>,
    newly_hashed: Vec<PendingBatchEvent>,
    next_hash_seq: u32,
    had_failure: bool,
    refreshed: Option<(String, DeviceSettings)>, // (hash_token, settings) from a GET /d/device refresh
    permanent_failures: Vec<(LogEntry, CoreError)>,
}

/// Phase 5a: the actual network calls, run without holding the state lock.
pub fn execute_hash_retries<A: ApiTransport>(plan: HashRetryPlan, api: &A) -> HashRetryOutcome {
    let HashRetryPlan {
        device_id,
        refresh_token,
        hash_base_url,
        cached_token,
        events,
        mut next_hash_seq,
        now_ms,
    } = plan;

    let mut refreshed: Option<(String, DeviceSettings)> = None;
    let needs_refresh = match &cached_token {
        None => true,
        Some((_, fetched_at)) => now_ms - fetched_at >= HASH_TOKEN_MAX_AGE_MS,
    };
    let mut hash_jwt = cached_token.map(|(token, _)| token).unwrap_or_default();
    if needs_refresh || hash_jwt.is_empty() {
        match api.get_device_settings(&refresh_token) {
            Ok(result) => {
                hash_jwt = result.hash_token.clone();
                refreshed = Some((result.hash_token, result.settings));
            }
            Err(err) => {
                log_warning(
                    "failed to refresh hash token, deferring hash retries",
                    Some(&err),
                );
                return HashRetryOutcome {
                    device_id,
                    still_pending: events,
                    newly_hashed: Vec::new(),
                    next_hash_seq,
                    had_failure: true,
                    refreshed: None,
                    permanent_failures: Vec::new(),
                };
            }
        }
    }

    let unix_time = (now_ms / 1000) as u32;
    let mut still_pending = Vec::new();
    let mut newly_hashed = Vec::new();
    let mut permanent_failures = Vec::new();
    let mut had_failure = false;
    let mut retried = 0usize;
    let mut stop = false;

    for event in events {
        if stop || retried >= MAX_HASH_RETRIES_PER_LOOP {
            still_pending.push(event);
            continue;
        }
        retried += 1;
        let encoded = match encode_batch_event(&event) {
            Ok(bytes) => bytes,
            Err(err) => {
                log_warning(
                    "encode_batch_event failed, keeping event for retry",
                    Some(&err),
                );
                still_pending.push(event);
                continue;
            }
        };
        let hash = compute_event_hash(&encoded);
        let seq = next_hash_seq.saturating_add(1);
        match api.upload_hash(hash_base_url.as_deref(), &hash_jwt, unix_time, seq, &hash) {
            Ok(()) => {
                next_hash_seq = seq;
                let is_high_risk = event
                    .risk
                    .is_some_and(|r| r >= crate::module::lifecycle::EXTRA_HIGH_RISK);
                let notify = is_high_risk.then(|| build_notify_payload(&event));
                let is_screenshot = matches!(event.event, UploadKind::Screenshot { .. });
                newly_hashed.push(PendingBatchEvent {
                    ts: event.ts,
                    risk: event.risk.unwrap_or(0.0),
                    encoded,
                    is_screenshot,
                    notify,
                });
            }
            Err(err) if err.is_bad_request() => {
                permanent_failures.push((event, err));
            }
            Err(err) if err.is_conflict() => {
                // Our local seq counter is behind the hash server's. seq isn't part
                // of the hash chain input, so retrying with a fresher seq is safe.
                had_failure = true;
                next_hash_seq = seq;
                log_warning(
                    "hash upload sequence conflict, advancing local seq and retrying",
                    Some(&err),
                );
                still_pending.push(event);
                stop = true;
            }
            Err(err) => {
                had_failure = true;
                log_warning("hash upload failed, will retry", Some(&err));
                still_pending.push(event);
                stop = true;
            }
        }
    }

    HashRetryOutcome {
        device_id,
        still_pending,
        newly_hashed,
        next_hash_seq,
        had_failure,
        refreshed,
        permanent_failures,
    }
}

/// Phase 5c: apply the hash-retry outcome.
pub fn commit_hash_retries(state: &mut UploadState, outcome: HashRetryOutcome, now_ms: i64) {
    // Defensive: apply_login/apply_logout only ever run between ticks, never
    // mid-tick, so this should always match — but drop silently rather than
    // corrupt a different session's queue if it somehow doesn't.
    if state
        .device_credentials
        .as_ref()
        .is_none_or(|c| c.device_id != outcome.device_id)
    {
        return;
    }

    // Splice unclaimed events back at the front so anything already queued
    // (or enqueued by this same tick's other requests) stays behind them.
    let mut pending = outcome.still_pending;
    pending.append(&mut state.pending_hash_events);
    state.pending_hash_events = pending;

    state.pending_batch_events.extend(outcome.newly_hashed);
    state.next_hash_seq = outcome.next_hash_seq;

    if let Some((hash_token, settings)) = outcome.refreshed {
        state.hash_token_cache = Some((hash_token, now_ms));
        state.settings = Some(settings);
    }

    for (_, err) in &outcome.permanent_failures {
        log_error(
            "hash upload failed permanently (400), dropping event",
            Some(err),
        );
    }

    if outcome.had_failure {
        state.hash_backoff.record_failure(now_ms);
    } else {
        state.hash_backoff.record_success();
    }
}

// ── Batch upload: plan / execute / commit ──────────────────────────────────

pub struct BatchPlan {
    device_id: String,
    refresh_token: String,
    batch: BatchUpload,
    count: usize,
}

/// Phase 4: decide whether a batch upload should run this tick.
pub fn plan_batch(
    state: &UploadState,
    now_ms: i64,
    batch_interval_ms: i64,
    screen_active: bool,
) -> Option<BatchPlan> {
    if state.pending_batch_events.is_empty() {
        return None;
    }
    if !(screen_active || state.bypass_lock) {
        return None;
    }
    let should = state.post_login_proof_batches_remaining > 0
        || state
            .last_batch_at_ms
            .map(|last| now_ms - last >= batch_interval_ms)
            .unwrap_or(true)
        || state.force_flush
        || state.pending_batch_events.len() >= MAX_BATCH_ITEMS_PER_UPLOAD;
    if !should || !state.batch_backoff.ready(now_ms) {
        return None;
    }
    let creds = state.device_credentials.as_ref()?;
    let settings = state.settings.as_ref().filter(|s| can_capture(s))?;
    let recipients = batch_recipients(settings).ok()?;

    let count = state
        .pending_batch_events
        .len()
        .min(MAX_BATCH_ITEMS_PER_UPLOAD);
    let mut items = state.pending_batch_events[..count].to_vec();
    items.sort_by_key(|event| event.ts);
    let start_time_ms = items[0].ts;
    let high_risk_count = items.iter().filter(|e| e.risk >= HIGH_RISK_RATING).count() as u32;
    let medium_risk_count = items
        .iter()
        .filter(|e| e.risk >= MEDIUM_RISK_RATING && e.risk < HIGH_RISK_RATING)
        .count() as u32;
    let screenshot_count = items.iter().filter(|e| e.is_screenshot).count() as u32;
    let notifications: Vec<NotifyPayload> = items.iter().filter_map(|e| e.notify.clone()).collect();
    let encoded: Vec<Vec<u8>> = items.into_iter().map(|event| event.encoded).collect();

    let batch = BatchBuilder::build_upload(
        &encoded,
        &CryptoEngine,
        &recipients,
        start_time_ms,
        now_ms,
        high_risk_count,
        medium_risk_count,
        screenshot_count,
        notifications,
    )
    .ok()?;

    Some(BatchPlan {
        device_id: creds.device_id.clone(),
        refresh_token: creds.refresh_token.clone(),
        batch,
        count,
    })
}

pub enum BatchOutcome {
    Uploaded {
        device_id: String,
        count: usize,
        response: UploadedBatchResponse,
    },
    LoggedOut {
        device_id: String,
    },
    Deferred {
        device_id: String,
    },
}

/// Phase 5b: the actual network call, run without holding the state lock.
pub fn execute_batch<A: ApiTransport>(plan: BatchPlan, api: &A) -> BatchOutcome {
    match api.upload_batch(&plan.refresh_token, &plan.batch) {
        Ok(response) => {
            tracing::info!(count = plan.count, "batch upload ok");
            BatchOutcome::Uploaded {
                device_id: plan.device_id,
                count: plan.count,
                response,
            }
        }
        Err(err) if err.is_not_found() || err.is_unauthorized() => {
            log_warning(
                "batch upload: device deregistered or unauth, logging out",
                Some(&err),
            );
            BatchOutcome::LoggedOut {
                device_id: plan.device_id,
            }
        }
        Err(err) => {
            log_warning("batch upload deferred", Some(&err));
            BatchOutcome::Deferred {
                device_id: plan.device_id,
            }
        }
    }
}

/// Phase 5c: apply the batch outcome. Returns `true` if the outcome
/// indicates the device should be logged out (deregistered or unauthorized)
/// — the caller (`Daemon`) performs the actual logout.
pub fn commit_batch(state: &mut UploadState, outcome: BatchOutcome, now_ms: i64) -> bool {
    let device_id = match &outcome {
        BatchOutcome::Uploaded { device_id, .. } => device_id,
        BatchOutcome::LoggedOut { device_id } => device_id,
        BatchOutcome::Deferred { device_id } => device_id,
    };
    if state
        .device_credentials
        .as_ref()
        .is_none_or(|c| &c.device_id != device_id)
    {
        return false;
    }

    match outcome {
        BatchOutcome::Uploaded {
            count, response, ..
        } => {
            state.pending_batch_events.drain(..count);
            if state.post_login_proof_batches_remaining > 0 {
                state.post_login_proof_batches_remaining -= 1;
            }
            state.last_batch_at_ms = Some(now_ms);
            // A landed batch makes the API reset the hash server's stored state
            // for this device, so the local seq counter must reset in lockstep.
            state.next_hash_seq = 0;
            state.settings = Some(response.settings);
            state.hash_token_cache = Some((response.hash_token, now_ms));
            state.batch_backoff.record_success();
            if state.pending_batch_events.is_empty() {
                state.force_flush = false;
                state.bypass_lock = false;
            }
            false
        }
        BatchOutcome::LoggedOut { .. } => true,
        BatchOutcome::Deferred { .. } => {
            state.batch_backoff.record_failure(now_ms);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MockApiClient;

    fn valid_credentials() -> DeviceCredentials {
        DeviceCredentials {
            device_id: "test-device".into(),
            refresh_token: "test-refresh".into(),
        }
    }

    fn valid_settings() -> DeviceSettings {
        DeviceSettings {
            device_id: "test-device".into(),
            name: "test device".into(),
            platform: "test".into(),
            wrapping_keys: vec![BatchRecipient {
                user_id: "test-user".into(),
                pub_key_base64: "CQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            }],
            hash_base_url: None,
        }
    }

    #[allow(clippy::field_reassign_with_default)]
    fn authenticated_state() -> UploadState {
        let mut state = UploadState::default();
        state.device_credentials = Some(valid_credentials());
        state.settings = Some(valid_settings());
        // Isolate tests from the post-login proof-batch mechanic, which would
        // otherwise force a batch upload regardless of interval/lock state.
        state.post_login_proof_batches_remaining = 0;
        state.last_batch_at_ms = Some(0);
        state
    }

    #[test]
    fn enqueue_without_credentials_is_a_noop() {
        let mut state = UploadState::default();
        enqueue(&mut state, 1_000, 0.0, UploadKind::CaptureFailed);
        assert!(state.pending_hash_events.is_empty());
    }

    #[test]
    fn enqueue_heartbeat_forces_flush_and_bypasses_lock() {
        let mut state = authenticated_state();
        enqueue(&mut state, 1_000, 0.0, UploadKind::Heartbeat);
        assert_eq!(state.pending_hash_events.len(), 1);
        assert!(state.force_flush);
        assert!(state.bypass_lock);
    }

    #[test]
    fn enqueue_high_risk_forces_flush_but_not_lock_bypass() {
        let mut state = authenticated_state();
        enqueue(
            &mut state,
            1_000,
            crate::module::lifecycle::EXTRA_HIGH_RISK,
            UploadKind::Alert {
                message: "test".into(),
            },
        );
        assert!(state.force_flush);
        assert!(!state.bypass_lock);
    }

    #[test]
    fn hash_retry_round_trip_moves_event_into_batch_queue() {
        let mut state = authenticated_state();
        enqueue(&mut state, 1_000, 0.0, UploadKind::CaptureFailed);

        let api = MockApiClient::new();
        let plan = plan_hash_retries(&mut state, 1_000, true).expect("expected a hash plan");
        let outcome = execute_hash_retries(plan, &api);
        assert!(outcome.still_pending.is_empty());
        assert_eq!(outcome.newly_hashed.len(), 1);
        commit_hash_retries(&mut state, outcome, 1_000);

        assert!(state.pending_hash_events.is_empty());
        assert_eq!(state.pending_batch_events.len(), 1);
        assert_eq!(state.next_hash_seq, 1);
    }

    #[test]
    fn batch_plan_none_when_lock_gate_blocks_and_not_bypassed() {
        let mut state = authenticated_state();
        state.pending_batch_events.push(PendingBatchEvent {
            ts: 0,
            risk: 0.0,
            encoded: vec![1, 2, 3],
            is_screenshot: false,
            notify: None,
        });
        assert!(plan_batch(&state, 1_000, 60_000, false).is_none());
    }

    #[test]
    fn batch_upload_round_trip_drains_queue_and_advances_state() {
        let mut state = authenticated_state();
        state.pending_batch_events.push(PendingBatchEvent {
            ts: 0,
            risk: 0.0,
            encoded: vec![1, 2, 3],
            is_screenshot: false,
            notify: None,
        });

        let api = MockApiClient::new();
        state.last_batch_at_ms = None;
        let plan = plan_batch(&state, 1_000, 60_000, true).expect("expected a batch plan");
        let outcome = execute_batch(plan, &api);
        let should_logout = commit_batch(&mut state, outcome, 1_000);
        assert!(!should_logout);
        assert!(state.pending_batch_events.is_empty());
        assert_eq!(state.next_hash_seq, 0);
    }

    #[test]
    fn commit_ignores_outcome_from_a_stale_session() {
        let mut state = authenticated_state();
        state.pending_batch_events.push(PendingBatchEvent {
            ts: 0,
            risk: 0.0,
            encoded: vec![1, 2, 3],
            is_screenshot: false,
            notify: None,
        });
        let api = MockApiClient::new();
        state.last_batch_at_ms = None;
        let plan = plan_batch(&state, 1_000, 60_000, true).expect("expected a batch plan");

        // Logout races the in-flight (unlocked) network call.
        state.reset_for_logout();

        let outcome = execute_batch(plan, &api);
        let should_logout = commit_batch(&mut state, outcome, 1_000);
        assert!(!should_logout);
        assert!(
            state.pending_batch_events.is_empty(),
            "logout already cleared the queue; the stale outcome must not resurrect it"
        );
    }
}
