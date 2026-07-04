mod batch;

use std::any::Any;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use batch::BatchBuilder;
pub(crate) use batch::MAX_BATCH_ITEMS_PER_UPLOAD;

use crate::api::ApiTransport;
use crate::crypto::{CryptoEngine, compute_event_hash, encode_batch_event};
use crate::error::{CoreError, CoreResult};
use crate::events::Ping;
use crate::events::bus::{Emitter, EventBus, Observer, StateType, log_error};
use crate::model::PartialStatus;
use crate::module::auth::{Login, Logout, LogoutRequested};
use crate::module::config::ConfigChanged;
use crate::module::lifecycle::ProcessStopped;
use crate::module::status::StatusRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Upload {
    pub risk: f32,
    pub kind: UploadKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlushBatchNow;
use crate::model::{
    BatchRecipient, BatchUpload, DeviceCredentials, DeviceSettings, LogEntry, NotifyPayload,
    ProcessStoppedReason, UploadKind,
};
use crate::platform::ScreenshotHooks;

pub(crate) const POST_LOGIN_PROOF_BATCH_COUNT: u32 = 3;

/// A hash-uploaded event awaiting batch upload, paired with its risk so the batch
/// upload can report how many high/medium-risk events it carries.
///
/// Serialized as `[ts, risk, encoded]`. For backward compatibility with state
/// written before #467 it also deserializes the legacy `[ts, encoded]` shape,
/// defaulting `risk` to `0.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingBatchEvent {
    pub ts: i64,
    pub risk: f32,
    pub encoded: Vec<u8>,
}

impl Serialize for PendingBatchEvent {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(3))?;
        seq.serialize_element(&self.ts)?;
        seq.serialize_element(&self.risk)?;
        seq.serialize_element(&self.encoded)?;
        seq.end()
    }
}

impl<'de> Deserialize<'de> for PendingBatchEvent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PendingBatchEventVisitor;

        impl<'de> serde::de::Visitor<'de> for PendingBatchEventVisitor {
            type Value = PendingBatchEvent;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a [ts, risk, encoded] or legacy [ts, encoded] sequence")
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<PendingBatchEvent, A::Error> {
                use serde::de::Error;
                let ts: i64 = seq
                    .next_element()?
                    .ok_or_else(|| A::Error::invalid_length(0, &self))?;
                // The second element distinguishes the two shapes: a number is the
                // risk (new form), an array is the encoded event (legacy form).
                let second: serde_json::Value = seq
                    .next_element()?
                    .ok_or_else(|| A::Error::invalid_length(1, &self))?;
                match second {
                    serde_json::Value::Array(_) => {
                        let encoded = serde_json::from_value(second).map_err(A::Error::custom)?;
                        Ok(PendingBatchEvent {
                            ts,
                            risk: 0.0,
                            encoded,
                        })
                    }
                    serde_json::Value::Number(number) => {
                        let risk = number.as_f64().unwrap_or(0.0) as f32;
                        let encoded: Vec<u8> = seq
                            .next_element()?
                            .ok_or_else(|| A::Error::invalid_length(2, &self))?;
                        Ok(PendingBatchEvent { ts, risk, encoded })
                    }
                    _ => Err(A::Error::custom(
                        "unexpected second element in pending batch event",
                    )),
                }
            }
        }

        deserializer.deserialize_seq(PendingBatchEventVisitor)
    }
}

/// Builds the notification payload for a high-risk event. Title/details are pulled
/// from the event body when present (e.g. `Dev` events); otherwise the server
/// derives a fallback title from the event type.
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

const MAX_HASH_RETRIES_PER_LOOP: usize = 8;
const MAX_NOTIFY_RETRIES_PER_LOOP: usize = 8;
const HASH_TOKEN_MAX_AGE: Duration = Duration::from_secs(55 * 60);

/// Risk-rating band thresholds mirroring `shared-web/risk.ts` (`getRiskRating`).
/// A batch reports how many of its events land in the high (>= 0.7) and medium
/// (0.4–0.7) bands so the server can summarize tamper activity in digest emails
/// without reading the encrypted payload.
const HIGH_RISK_RATING: f32 = 0.7;
const MEDIUM_RISK_RATING: f32 = 0.4;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct UploadObserverState {
    pub pending_batch_events: Vec<PendingBatchEvent>,
    pub pending_hash_events: Vec<LogEntry>,
    #[serde(default)]
    pub pending_notify_events: Vec<NotifyPayload>,
    pub last_batch_at_ms: Option<i64>,
    pub post_login_proof_batches_remaining: u32,
    #[serde(default)]
    pub settings: Option<DeviceSettings>,
    #[serde(default)]
    pub device_credentials: Option<DeviceCredentials>,
}

impl UploadObserverState {
    pub fn reset_for_login(&mut self) {
        self.pending_batch_events.clear();
        self.pending_hash_events.clear();
        self.pending_notify_events.clear();
        self.last_batch_at_ms = None;
        self.post_login_proof_batches_remaining = POST_LOGIN_PROOF_BATCH_COUNT;
    }

    pub fn reset_for_logout(&mut self) {
        self.pending_batch_events.clear();
        self.pending_hash_events.clear();
        self.pending_notify_events.clear();
        self.last_batch_at_ms = None;
        self.post_login_proof_batches_remaining = 0;
        self.settings = None;
        self.device_credentials = None;
    }

    pub fn pending_request_count(&self) -> usize {
        self.pending_hash_events.len()
            + self.pending_notify_events.len()
            + usize::from(!self.pending_batch_events.is_empty())
    }
}

/// Drains a retry queue, calling `try_one` on each item up to `max_retries` times per call.
fn drain_retry_queue<T>(
    events: Vec<T>,
    max_retries: usize,
    mut try_one: impl FnMut(T) -> Option<T>,
) -> Vec<T> {
    let mut still_pending = Vec::with_capacity(events.len());
    let mut stop = false;
    let mut retried = 0;
    for event in events {
        if stop || retried >= max_retries {
            still_pending.push(event);
            continue;
        }
        retried += 1;
        if let Some(e) = try_one(event) {
            still_pending.push(e);
            stop = true;
        }
    }
    still_pending
}

pub struct UploadModule<A: ApiTransport + Clone + Send + Sync + 'static> {
    pub state: UploadObserverState,
    api: A,
    hash_token_cache: Option<(String, Instant)>,
    pub batch_interval_ms: i64,
    platform: Box<dyn ScreenshotHooks>,
    pub authenticated: bool,
}

impl<A: ApiTransport + Clone + Send + Sync + 'static> UploadModule<A> {
    pub fn new(platform: Box<dyn ScreenshotHooks>, api: A, batch_interval_ms: i64) -> Self {
        Self {
            state: UploadObserverState::default(),
            api,
            hash_token_cache: None,
            batch_interval_ms,
            platform,
            authenticated: false,
        }
    }

    fn upload_batch(&self, batch: &BatchUpload) -> CoreResult<()> {
        let creds = self
            .state
            .device_credentials
            .as_ref()
            .ok_or(CoreError::NotAuthenticated)?;
        self.api
            .upload_batch(&creds.refresh_token, batch)
            .map(|_| ())
    }

    fn notify(&self, payload: &NotifyPayload) -> CoreResult<()> {
        let creds = self
            .state
            .device_credentials
            .as_ref()
            .ok_or(CoreError::NotAuthenticated)?;
        self.api.notify(&creds.refresh_token, payload).map(|_| ())
    }

    fn upload_hash(
        &mut self,
        hash_base_url: Option<&str>,
        content_hash: &[u8; 32],
    ) -> CoreResult<()> {
        let hash_jwt = self.ensure_hash_token()?;
        self.api.upload_hash(hash_base_url, &hash_jwt, content_hash)
    }

    /// Fetches a hash-server JWT, caching it for [`HASH_TOKEN_MAX_AGE`] so we don't hit
    /// `POST /d/token` on every hash upload. Cleared on login/logout.
    fn ensure_hash_token(&mut self) -> CoreResult<String> {
        let refresh_token = self
            .state
            .device_credentials
            .as_ref()
            .ok_or(CoreError::NotAuthenticated)?
            .refresh_token
            .clone();

        let needs_refresh = match &self.hash_token_cache {
            None => true,
            Some((_, fetched_at)) => fetched_at.elapsed() >= HASH_TOKEN_MAX_AGE,
        };

        if needs_refresh {
            let token = self.api.get_hash_token(&refresh_token)?;
            self.hash_token_cache = Some((token.clone(), Instant::now()));
            Ok(token)
        } else {
            Ok(self.hash_token_cache.as_ref().unwrap().0.clone())
        }
    }

    fn retry_pending_hashes(&mut self) -> CoreResult<()> {
        let hash_base_url = self
            .state
            .settings
            .as_ref()
            .and_then(|s| s.hash_base_url.clone());
        let events = std::mem::take(&mut self.state.pending_hash_events);
        self.state.pending_hash_events =
            drain_retry_queue(events, MAX_HASH_RETRIES_PER_LOOP, |event| {
                let encoded = match encode_batch_event(&event) {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        log_error(
                            "encode_batch_event failed, keeping event for retry",
                            Some(&err),
                        );
                        return Some(event);
                    }
                };
                let hash = compute_event_hash(&encoded);
                match self.upload_hash(hash_base_url.as_deref(), &hash) {
                    Ok(()) => {
                        self.state.pending_batch_events.push(PendingBatchEvent {
                            ts: event.ts,
                            risk: event.risk.unwrap_or(0.0),
                            encoded,
                        });
                        None
                    }
                    Err(_) => Some(event),
                }
            });
        Ok(())
    }

    fn retry_pending_notifies(&mut self) -> CoreResult<()> {
        let events = std::mem::take(&mut self.state.pending_notify_events);
        self.state.pending_notify_events =
            drain_retry_queue(events, MAX_NOTIFY_RETRIES_PER_LOOP, |payload| {
                match self.notify(&payload) {
                    Ok(()) => None,
                    Err(err) if err.is_bad_request() => {
                        log_error("notify failed permanently", Some(&err));
                        None
                    }
                    Err(_) => Some(payload),
                }
            });
        Ok(())
    }

    fn maybe_upload_batch(&mut self, now_ms: i64, emitter: &Emitter) -> CoreResult<()> {
        // Whether we have recipients to wrap for is decided in `try_upload_batch`
        // after refetching settings, not from the (possibly stale) cached copy.
        if self.state.pending_batch_events.is_empty() {
            return Ok(());
        }
        let should = self.state.post_login_proof_batches_remaining > 0
            || self
                .state
                .last_batch_at_ms
                .map(|last| now_ms - last >= self.batch_interval_ms)
                .unwrap_or(true)
            || self.state.pending_batch_events.len() >= MAX_BATCH_ITEMS_PER_UPLOAD;
        if should {
            self.try_upload_batch(now_ms, emitter)?;
        }
        Ok(())
    }

    /// Refetches device settings from the API immediately before a batch upload
    /// so the batch key is wrapped for the current recipient set (e.g. a partner
    /// added or removed since the last periodic refresh). Returns `false` when the
    /// device is gone and the batch should be abandoned; a transient failure
    /// returns `true` so the batch still uploads against the last known settings.
    fn refresh_settings_before_batch(&mut self, emitter: &Emitter) -> bool {
        let Some(creds) = self.state.device_credentials.as_ref() else {
            return true;
        };
        let refresh_token = creds.refresh_token.clone();
        match self.api.get_device_settings(&refresh_token) {
            Ok(settings) => {
                self.state.settings = Some(settings);
                true
            }
            Err(err) if err.is_not_found() || err.is_unauthorized() => {
                log_error(
                    "settings refresh before batch: device deregistered or unauth, logging out",
                    Some(&err),
                );
                let _ = emitter.send(LogoutRequested);
                false
            }
            Err(err) => {
                log_error(
                    "settings refresh before batch failed; using last known settings",
                    Some(&err),
                );
                true
            }
        }
    }

    pub(crate) fn try_upload_batch(&mut self, now_ms: i64, emitter: &Emitter) -> CoreResult<()> {
        if self.state.pending_batch_events.is_empty() {
            return Ok(());
        }
        if !self.refresh_settings_before_batch(emitter) {
            return Ok(());
        }
        // With freshly fetched settings in hand, only proceed when there is at
        // least one recipient to wrap for; otherwise keep the events queued.
        let settings = match self.state.settings.as_ref() {
            Some(s) if can_capture(s) => s,
            _ => return Ok(()),
        };
        let count = self
            .state
            .pending_batch_events
            .len()
            .min(MAX_BATCH_ITEMS_PER_UPLOAD);
        let mut items = self.state.pending_batch_events[..count].to_vec();
        items.sort_by_key(|event| event.ts);
        let start_time_ms = items[0].ts;
        let high_risk_count = items.iter().filter(|e| e.risk >= HIGH_RISK_RATING).count() as u32;
        let medium_risk_count = items
            .iter()
            .filter(|e| e.risk >= MEDIUM_RISK_RATING && e.risk < HIGH_RISK_RATING)
            .count() as u32;
        let encoded: Vec<Vec<u8>> = items.into_iter().map(|event| event.encoded).collect();
        let recipients = batch_recipients(settings)?;
        let batch = BatchBuilder::build_upload(
            &encoded,
            &CryptoEngine,
            &recipients,
            start_time_ms,
            now_ms,
            high_risk_count,
            medium_risk_count,
        )?;
        match self.upload_batch(&batch) {
            Ok(_) => {
                #[cfg(debug_assertions)]
                eprintln!("[upload] batch ok: {count} events, start_ms={start_time_ms}");
                self.state.pending_batch_events.drain(..count);
                if self.state.post_login_proof_batches_remaining > 0 {
                    self.state.post_login_proof_batches_remaining -= 1;
                }
                self.state.last_batch_at_ms = Some(now_ms);
                Ok(())
            }
            Err(err) if err.is_not_found() || err.is_unauthorized() => {
                log_error(
                    "batch upload: device deregistered or unauth, logging out",
                    Some(&err),
                );
                let _ = emitter.send(LogoutRequested);
                Ok(())
            }
            Err(err) => {
                log_error("batch upload deferred", Some(&err));
                Ok(())
            }
        }
    }

    fn handle_ping(&mut self, emitter: &Emitter) -> CoreResult<()> {
        if !self.authenticated {
            return Ok(());
        }
        // Battery: don't wake the network/radio while the screen is off/locked. Queued
        // events persist and flush on the next ping once the screen is active.
        if self.platform.is_locked_or_screensaver()? {
            return Ok(());
        }
        let now_ms = self.platform.get_time_utc_ms()?;

        if let Some(last) = self.state.last_batch_at_ms
            && now_ms < last
        {
            self.state.last_batch_at_ms = None;
        }

        self.retry_pending_hashes()?;
        // Force the batch out before sending any queued notify emails so the
        // event(s) they reference already exist server-side by the time the
        // email goes out, rather than waiting on the interval timer.
        if self.state.pending_notify_events.is_empty() {
            self.maybe_upload_batch(now_ms, emitter)?;
        } else {
            self.try_upload_batch(now_ms, emitter)?;
        }
        self.retry_pending_notifies()?;
        Ok(())
    }

    fn handle_upload(
        &mut self,
        risk: f32,
        kind: crate::model::UploadKind,
        emitter: &Emitter,
    ) -> CoreResult<()> {
        if !self.authenticated {
            return Ok(());
        }
        let now_ms = self.platform.get_time_utc_ms()?;
        // Heartbeats bypass the screen-lock gate: the whole point is to prove the
        // device is alive when idle, so we upload even if the screen is locked.
        let is_heartbeat = matches!(kind, UploadKind::Heartbeat);
        let is_high_risk = risk >= crate::module::lifecycle::EXTRA_HIGH_RISK;
        let entry = LogEntry {
            ts: now_ms,
            risk: Some(risk),
            event: kind,
        };
        // High-risk events trigger an immediate email notification, but the event
        // body still rides through the hash chain + encrypted batch below — the
        // server never receives it unencrypted.
        if is_high_risk {
            self.state
                .pending_notify_events
                .push(build_notify_payload(&entry));
        }
        // Always enqueue, but only attempt network I/O while the screen is active
        // (battery): a locked/off screen leaves events queued for the next ping flush.
        self.state.pending_hash_events.push(entry);
        let screen_active = !self.platform.is_locked_or_screensaver()?;
        if screen_active || is_heartbeat {
            self.retry_pending_hashes()?;
            if is_heartbeat || is_high_risk {
                // Force an immediate batch flush — don't wait for the interval timer.
                // For a high-risk event this also guarantees the encrypted event is
                // uploaded before the notify email below goes out, so the event
                // already exists server-side when the recipient follows the link.
                self.try_upload_batch(now_ms, emitter)?;
            } else {
                self.maybe_upload_batch(now_ms, emitter)?;
            }
        }
        if is_high_risk && screen_active {
            self.retry_pending_notifies()?;
        }
        Ok(())
    }
}

impl<A: ApiTransport + Clone + Send + Sync + 'static> Observer for UploadModule<A> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "upload"
    }

    fn init(&mut self, _bus: &mut EventBus, state: StateType) -> CoreResult<()> {
        if !state.is_null() {
            self.state = serde_json::from_value(state)?;
            if self.state.device_credentials.is_some() {
                self.authenticated = true;
            }
        }
        Ok(())
    }

    fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
        crate::dispatch_event!(event, {
            ev: Login => {
                self.authenticated = true;
                self.hash_token_cache = None;
                self.state.settings = Some(ev.settings.clone());
                self.state.device_credentials = Some(ev.credentials.clone());
                self.state.reset_for_login();
                Ok(())
            },
            _: Logout => {
                self.authenticated = false;
                self.hash_token_cache = None;
                self.state.reset_for_logout();
                Ok(())
            },
            _: StatusRequest => {
                let _ = emitter.send(PartialStatus::Upload {
                    pending_request_count: self.state.pending_request_count(),
                });
                Ok(())
            },
            _: Ping => self.handle_ping(emitter),
            ev: ProcessStopped => {
                if matches!(ev.0, ProcessStoppedReason::Shutdown)
                    && self.authenticated
                    && !self.state.pending_batch_events.is_empty()
                {
                    let now_ms = self.platform.get_time_utc_ms()?;
                    let _ = self.try_upload_batch(now_ms, emitter);
                }
                Ok(())
            },
            ev: Upload => self.handle_upload(ev.risk, ev.kind.clone(), emitter),
            _: FlushBatchNow => {
                if self.authenticated {
                    let now_ms = self.platform.get_time_utc_ms()?;
                    self.try_upload_batch(now_ms, emitter)?;
                }
                Ok(())
            },
            ev: ConfigChanged => {
                self.batch_interval_ms = ev.batch_interval_ms as i64;
                Ok(())
            },
        })
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.state)?)
    }
}

fn can_capture(settings: &DeviceSettings) -> bool {
    !settings.wrapping_keys.is_empty()
}

fn batch_recipients(settings: &DeviceSettings) -> CoreResult<Vec<BatchRecipient>> {
    use crate::error::CoreError;
    if settings.wrapping_keys.is_empty() {
        return Err(CoreError::InvalidState("no batch recipients available"));
    }
    Ok(settings.wrapping_keys.clone())
}

#[cfg(test)]
mod tests {
    use super::{FlushBatchNow, Upload, UploadModule};
    use crate::events::Ping;
    use crate::model::{
        BatchRecipient, DeviceCredentials, DeviceSettings, LogEntry, PartialStatus,
        ProcessStoppedReason, ScreenshotSkipReason, UploadKind,
    };
    use crate::module::auth::{Login, Logout};
    use crate::module::lifecycle::ProcessStopped;
    use crate::module::status::StatusRequest;
    use crate::testing::{EventTester, MockApiClient};

    fn skipped_upload() -> Upload {
        Upload {
            risk: 0.0,
            kind: UploadKind::ScreenshotSkipped {
                reason: ScreenshotSkipReason::StaticScreen,
            },
        }
    }

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

    fn login_event() -> Login {
        Login {
            credentials: valid_credentials(),
            settings: valid_settings(),
        }
    }

    #[test]
    fn upload_when_unauthenticated_is_silently_ignored() {
        let mut b = EventTester::builder();
        b.add(UploadModule::new(Box::new(b.platform()), b.api(), 60_000));
        let mut t = b.build();
        t.emit(
            1,
            Upload {
                risk: 0.0,
                kind: UploadKind::Dev {
                    title: "ignored".into(),
                    details: None,
                },
            },
        );
        assert!(t.api.state().hash_uploads.is_empty());
        assert_eq!(
            t.observer::<UploadModule<MockApiClient>>()
                .state
                .pending_hash_events
                .len(),
            0
        );
    }

    #[test]
    fn login_sets_authenticated_credentials_and_settings() {
        let mut b = EventTester::builder();
        b.add(UploadModule::new(Box::new(b.platform()), b.api(), 60_000));
        let mut t = b.build();
        t.emit(1, login_event());
        let m = t.observer::<UploadModule<MockApiClient>>();
        assert!(
            m.state.settings.is_some(),
            "login should set device settings"
        );
        assert!(
            m.state.device_credentials.is_some(),
            "login should set credentials"
        );
        assert_eq!(m.state.post_login_proof_batches_remaining, 3);
    }

    #[test]
    fn logout_clears_authenticated_state() {
        let mut b = EventTester::builder();
        b.add(UploadModule::new(Box::new(b.platform()), b.api(), 60_000));
        let mut t = b.build();
        t.emit(1, login_event());
        t.emit(1, Logout);
        let m = t.observer::<UploadModule<MockApiClient>>();
        assert!(m.state.settings.is_none(), "logout should clear settings");
        assert!(
            m.state.device_credentials.is_none(),
            "logout should clear credentials"
        );
        assert!(
            m.state.pending_hash_events.is_empty(),
            "logout should clear pending events"
        );
        assert!(
            m.state.pending_batch_events.is_empty(),
            "logout should clear batch queue"
        );
    }

    #[test]
    fn hash_token_is_fetched_once_and_cached_across_uploads() {
        let mut b = EventTester::builder();
        b.add(UploadModule::new(Box::new(b.platform()), b.api(), 60_000));
        let mut t = b.build();
        t.emit(1, login_event());

        // Two low-risk uploads both flush through the hash server on an active screen.
        t.emit(2, skipped_upload());
        t.emit(3, skipped_upload());

        let s = t.api.state();
        assert_eq!(s.hash_uploads.len(), 2, "both hashes should upload");
        assert_eq!(
            s.get_hash_token_calls.len(),
            1,
            "hash-server token should be fetched once and cached"
        );
    }

    #[test]
    fn settings_are_refetched_before_batch_and_new_recipients_are_used() {
        let mut b = EventTester::builder();
        b.add(UploadModule::new(Box::new(b.platform()), b.api(), 60_000));
        let mut t = b.build();
        // Login seeds settings with a single recipient (the owner).
        t.emit(1, login_event());

        // A partner is added server-side: the next settings fetch returns two recipients.
        t.api.program_get_device_settings(Ok(DeviceSettings {
            device_id: "test-device".into(),
            name: "test device".into(),
            platform: "test".into(),
            wrapping_keys: vec![
                BatchRecipient {
                    user_id: "test-user".into(),
                    pub_key_base64: "CQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
                },
                BatchRecipient {
                    user_id: "partner-user".into(),
                    pub_key_base64: "CQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
                },
            ],
            hash_base_url: None,
        }));

        // A low-risk upload flushes a batch (post-login proof batches force it out).
        t.emit(2, skipped_upload());

        let s = t.api.state();
        assert_eq!(
            s.get_device_settings_calls.len(),
            1,
            "settings should be refetched once, right before the batch upload"
        );
        assert_eq!(s.batch_uploads.len(), 1, "batch should upload");
        let recipients: Vec<String> = s.batch_uploads[0]
            .batch
            .access_keys
            .iter()
            .map(|k| k.user_id.clone())
            .collect();
        assert_eq!(
            recipients,
            vec!["test-user".to_string(), "partner-user".to_string()],
            "batch should be wrapped for the freshly fetched recipient set"
        );
    }

    #[test]
    fn status_request_emits_pending_request_count() {
        let mut b = EventTester::builder();
        b.add(UploadModule::new(Box::new(b.platform()), b.api(), 60_000));
        let mut t = b.build();
        {
            let m = t.observer::<UploadModule<MockApiClient>>();
            m.authenticated = true;
            m.state.device_credentials = Some(valid_credentials());
            m.state.post_login_proof_batches_remaining = 0;
            m.state.last_batch_at_ms = Some(1_000);
            m.state.pending_hash_events.push(LogEntry {
                ts: 0,
                risk: Some(0.0),
                event: UploadKind::Dev {
                    title: "a".into(),
                    details: None,
                },
            });
            m.state
                .pending_batch_events
                .push(crate::module::upload::PendingBatchEvent {
                    ts: 500,
                    risk: 0.0,
                    encoded: vec![1, 2, 3],
                });
        }
        t.emit(1, StatusRequest);
        t.assert_like::<PartialStatus>(crate::like!(PartialStatus::Upload {
            pending_request_count: 2,
        }));
    }

    #[test]
    fn locked_screen_defers_uploads_until_active() {
        let mut b = EventTester::builder();
        b.platform().set_locked_or_screensaver(true);
        b.add(UploadModule::new(Box::new(b.platform()), b.api(), 60_000));
        let mut t = b.build();
        t.emit(1, login_event());

        // While locked, the event is queued but no network call fires on Upload or Ping.
        t.emit(1, skipped_upload());
        t.emit(2, Ping);
        {
            let s = t.api.state();
            assert!(s.hash_uploads.is_empty(), "no hash upload while locked");
            assert!(s.batch_uploads.is_empty(), "no batch upload while locked");
            assert!(
                s.notify_calls.is_empty(),
                "no direct log upload while locked"
            );
        }
        assert_eq!(
            t.observer::<UploadModule<MockApiClient>>()
                .state
                .pending_hash_events
                .len(),
            1,
            "event should remain queued while locked"
        );

        // Screen active again → next ping flushes the queue.
        t.platform.set_locked_or_screensaver(false);
        t.emit(3, Ping);
        let s = t.api.state();
        assert!(!s.hash_uploads.is_empty(), "hash flushes once active");
        assert!(!s.batch_uploads.is_empty(), "batch flushes once active");
    }

    #[test]
    fn flush_batch_now_uploads_while_locked() {
        let mut b = EventTester::builder();
        b.platform().set_locked_or_screensaver(true);
        b.add(UploadModule::new(Box::new(b.platform()), b.api(), 60_000));
        let mut t = b.build();
        t.emit(1, login_event());
        t.observer::<UploadModule<MockApiClient>>()
            .state
            .pending_batch_events
            .push(crate::module::upload::PendingBatchEvent {
                ts: 500,
                risk: 0.0,
                encoded: vec![1, 2, 3],
            });

        // A plain ping does not flush while locked.
        t.emit(2, Ping);
        assert!(t.api.state().batch_uploads.is_empty());

        // FlushBatchNow is an explicit flush path → uploads regardless of lock.
        t.emit(3, FlushBatchNow);
        assert_eq!(t.api.state().batch_uploads.len(), 1);
    }

    #[test]
    fn heartbeat_upload_bypasses_lock_and_forces_batch_flush() {
        let mut b = EventTester::builder();
        b.platform().set_locked_or_screensaver(true);
        b.add(UploadModule::new(Box::new(b.platform()), b.api(), 60_000));
        let mut t = b.build();
        t.emit(1, login_event());

        // With a locked screen a normal Upload is queued but not flushed.
        t.emit(2, skipped_upload());
        t.emit(3, Ping);
        assert!(
            t.api.state().batch_uploads.is_empty(),
            "no batch while locked"
        );

        // A Heartbeat Upload bypasses the lock gate and forces the batch out.
        t.emit(
            4,
            Upload {
                risk: 0.0,
                kind: UploadKind::Heartbeat,
            },
        );
        assert!(
            !t.api.state().batch_uploads.is_empty(),
            "heartbeat should flush batch even while locked"
        );
    }

    #[test]
    fn extra_high_risk_upload_forces_batch_flush_regardless_of_interval() {
        let mut b = EventTester::builder();
        // Huge interval so `maybe_upload_batch` would never fire on its own.
        b.add(UploadModule::new(
            Box::new(b.platform()),
            b.api(),
            24 * 60 * 60 * 1_000,
        ));
        let mut t = b.build();
        t.emit(1, login_event());

        t.emit(
            2,
            Upload {
                risk: crate::module::lifecycle::EXTRA_HIGH_RISK,
                kind: UploadKind::Alert {
                    message: "tamper detected".into(),
                },
            },
        );

        assert_eq!(
            t.api.state().batch_uploads.len(),
            1,
            "extra-high-risk upload should force an immediate batch flush"
        );
        assert_eq!(
            t.api.state().notify_calls.len(),
            1,
            "extra-high-risk upload should still send the notify email"
        );
        assert!(
            t.observer::<UploadModule<MockApiClient>>()
                .state
                .pending_batch_events
                .is_empty(),
            "the event should have been uploaded, not left queued"
        );
    }

    #[test]
    fn ping_forces_batch_flush_before_sending_queued_notify() {
        let mut b = EventTester::builder();
        b.platform().set_locked_or_screensaver(true);
        b.add(UploadModule::new(
            Box::new(b.platform()),
            b.api(),
            24 * 60 * 60 * 1_000,
        ));
        let mut t = b.build();
        t.emit(1, login_event());

        // Extra-high-risk event arrives while locked: queued for later, no
        // network I/O yet (matches the existing locked-screen deferral).
        t.emit(
            2,
            Upload {
                risk: crate::module::lifecycle::EXTRA_HIGH_RISK,
                kind: UploadKind::Alert {
                    message: "tamper detected".into(),
                },
            },
        );
        assert!(t.api.state().batch_uploads.is_empty());
        assert!(t.api.state().notify_calls.is_empty());

        // Screen unlocks; the next ping should flush the batch before sending
        // the queued notify, even though the batch interval hasn't elapsed.
        t.platform.set_locked_or_screensaver(false);
        t.emit(3, Ping);

        assert_eq!(
            t.api.state().batch_uploads.len(),
            1,
            "ping should force the batch out ahead of the queued notify"
        );
        assert_eq!(
            t.api.state().notify_calls.len(),
            1,
            "queued notify should still go out"
        );
    }

    #[test]
    fn shutdown_flush_uploads_while_locked() {
        let mut b = EventTester::builder();
        b.platform().set_locked_or_screensaver(true);
        b.add(UploadModule::new(Box::new(b.platform()), b.api(), 60_000));
        let mut t = b.build();
        t.emit(1, login_event());
        t.observer::<UploadModule<MockApiClient>>()
            .state
            .pending_batch_events
            .push(crate::module::upload::PendingBatchEvent {
                ts: 500,
                risk: 0.0,
                encoded: vec![1, 2, 3],
            });

        // ProcessStopped(Shutdown) is a terminal flush path → uploads regardless of lock.
        t.emit(2, ProcessStopped(ProcessStoppedReason::Shutdown));
        assert_eq!(t.api.state().batch_uploads.len(), 1);
    }
}
