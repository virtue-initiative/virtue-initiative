pub mod api;
mod batch;

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

pub use api::UploadApi;
use batch::BatchBuilder;
pub(crate) use batch::MAX_BATCH_ITEMS_PER_UPLOAD;

use crate::api::ApiTransport;
use crate::crypto::{CryptoEngine, compute_event_hash, encode_batch_event};
use crate::error::CoreResult;
use crate::events::bus::{Emitter, EventBus, Observer, StateType, log_error};
use crate::events::types::{
    ConfigChanged, DeviceSettingsRefreshed, FlushBatchNow, Login, Logout, LogoutRequested,
    PartialStatus, Ping, ProcessStopped, StatusRequest, Upload,
};
use crate::model::{
    BatchRecipient, DeviceCredentials, DeviceSettings, LogEntry, ProcessStoppedReason,
};
use crate::platform::ScreenshotHooks;

pub(crate) const POST_LOGIN_PROOF_BATCH_COUNT: u32 = 3;

const MAX_HASH_RETRIES_PER_LOOP: usize = 8;
const MAX_DIRECT_LOG_RETRIES_PER_LOOP: usize = 8;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct UploadObserverState {
    pub pending_batch_events: Vec<(i64, Vec<u8>)>,
    pub pending_hash_events: Vec<LogEntry>,
    pub pending_immediate_events: Vec<LogEntry>,
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
        self.pending_immediate_events.clear();
        self.last_batch_at_ms = None;
        self.post_login_proof_batches_remaining = POST_LOGIN_PROOF_BATCH_COUNT;
    }

    pub fn reset_for_logout(&mut self) {
        self.pending_batch_events.clear();
        self.pending_hash_events.clear();
        self.pending_immediate_events.clear();
        self.last_batch_at_ms = None;
        self.post_login_proof_batches_remaining = 0;
        self.settings = None;
        self.device_credentials = None;
    }

    pub fn pending_request_count(&self) -> usize {
        self.pending_hash_events.len()
            + self.pending_immediate_events.len()
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

pub struct UploadInner<A: ApiTransport + Clone> {
    pub state: UploadObserverState,
    pub upload_api: UploadApi<A>,
    pub batch_interval_ms: i64,
    platform: Box<dyn ScreenshotHooks>,
    pub authenticated: bool,
}

impl<A: ApiTransport + Clone + 'static> UploadInner<A> {
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
                match self.upload_api.upload_hash(hash_base_url.as_deref(), &hash) {
                    Ok(()) => {
                        self.state.pending_batch_events.push((event.ts, encoded));
                        None
                    }
                    Err(_) => Some(event),
                }
            });
        Ok(())
    }

    fn retry_pending_immediates(&mut self) -> CoreResult<()> {
        let events = std::mem::take(&mut self.state.pending_immediate_events);
        self.state.pending_immediate_events =
            drain_retry_queue(events, MAX_DIRECT_LOG_RETRIES_PER_LOOP, |entry| match self
                .upload_api
                .upload_log(&entry)
            {
                Ok(()) => None,
                Err(err) if err.is_bad_request() => {
                    log_error("direct log upload failed permanently", Some(&err));
                    None
                }
                Err(_) => Some(entry),
            });
        Ok(())
    }

    fn maybe_upload_batch(&mut self, now_ms: i64, emitter: &Emitter) -> CoreResult<()> {
        let can = self.state.settings.as_ref().is_some_and(can_capture);
        if self.state.pending_batch_events.is_empty() || !can {
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

    pub(crate) fn try_upload_batch(&mut self, now_ms: i64, emitter: &Emitter) -> CoreResult<()> {
        if self.state.pending_batch_events.is_empty() {
            return Ok(());
        }
        let settings = match self.state.settings.as_ref() {
            Some(s) => s,
            None => return Ok(()),
        };
        let count = self
            .state
            .pending_batch_events
            .len()
            .min(MAX_BATCH_ITEMS_PER_UPLOAD);
        let mut items = self.state.pending_batch_events[..count].to_vec();
        items.sort_by_key(|(ts, _)| *ts);
        let start_time_ms = items[0].0;
        let encoded: Vec<Vec<u8>> = items.into_iter().map(|(_, bytes)| bytes).collect();
        let recipients = batch_recipients(settings)?;
        let batch = BatchBuilder::build_upload(
            &encoded,
            &CryptoEngine,
            &recipients,
            start_time_ms,
            now_ms,
        )?;
        match self.upload_api.upload_batch(&batch) {
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
        let now_ms = self.platform.get_time_utc_ms()?;

        if let Some(last) = self.state.last_batch_at_ms {
            if now_ms < last {
                self.state.last_batch_at_ms = None;
            }
        }

        self.retry_pending_hashes()?;
        self.retry_pending_immediates()?;
        self.maybe_upload_batch(now_ms, emitter)?;
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
        let entry = LogEntry {
            ts: now_ms,
            risk: Some(risk),
            event: kind,
        };
        if risk >= crate::module::lifecycle::HIGH_RISK_LIFECYCLE_ALERT {
            self.state.pending_immediate_events.push(entry);
            self.retry_pending_immediates()?;
        } else {
            self.state.pending_hash_events.push(entry);
            self.retry_pending_hashes()?;
            self.maybe_upload_batch(now_ms, emitter)?;
        }
        Ok(())
    }
}

pub struct UploadModule<A: ApiTransport + Clone + Send + Sync + 'static> {
    pub inner: Arc<Mutex<UploadInner<A>>>,
}

impl<A: ApiTransport + Clone + Send + Sync + 'static> UploadModule<A> {
    pub fn new(platform: Box<dyn ScreenshotHooks>, api: A, batch_interval_ms: i64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(UploadInner {
                state: UploadObserverState::default(),
                upload_api: UploadApi::new(api),
                batch_interval_ms,
                platform,
                authenticated: false,
            })),
        }
    }
}

impl<A: ApiTransport + Clone + Send + Sync + 'static> Observer for UploadModule<A> {
    fn name(&self) -> &'static str {
        "upload"
    }

    fn init(&mut self, bus: &mut EventBus, state: StateType) -> CoreResult<()> {
        if !state.is_null() {
            let mut g = self.inner.lock().unwrap();
            g.state = serde_json::from_value(state)?;
            if let Some(creds) = g.state.device_credentials.clone() {
                g.upload_api.set_credentials(Some(creds));
                g.authenticated = true;
            }
        }

        let emitter = bus.emitter();

        let inner = Arc::clone(&self.inner);
        bus.subscribe(move |ev: &Login| {
            let mut g = inner.lock().unwrap();
            g.authenticated = true;
            g.upload_api.set_credentials(Some(ev.credentials.clone()));
            g.state.settings = Some(ev.settings.clone());
            g.state.device_credentials = Some(ev.credentials.clone());
            g.state.reset_for_login();
            Ok(())
        });

        let inner = Arc::clone(&self.inner);
        bus.subscribe(move |_: &Logout| {
            let mut g = inner.lock().unwrap();
            g.authenticated = false;
            g.upload_api.set_credentials(None);
            g.state.reset_for_logout();
            Ok(())
        });

        let inner = Arc::clone(&self.inner);
        bus.subscribe(move |ev: &DeviceSettingsRefreshed| {
            inner.lock().unwrap().state.settings = Some(ev.settings.clone());
            Ok(())
        });

        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |_: &StatusRequest| {
            let g = inner.lock().unwrap();
            let _ = e.send(PartialStatus::Upload {
                pending_request_count: g.state.pending_request_count(),
            });
            Ok(())
        });

        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |_: &Ping| inner.lock().unwrap().handle_ping(&e));

        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |ev: &ProcessStopped| {
            if matches!(ev.0, ProcessStoppedReason::Shutdown) {
                let mut g = inner.lock().unwrap();
                if g.authenticated && !g.state.pending_batch_events.is_empty() {
                    let now_ms = g.platform.get_time_utc_ms()?;
                    let _ = g.try_upload_batch(now_ms, &e);
                }
            }
            Ok(())
        });

        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |ev: &Upload| {
            inner
                .lock()
                .unwrap()
                .handle_upload(ev.risk, ev.kind.clone(), &e)
        });

        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |_: &FlushBatchNow| {
            let mut g = inner.lock().unwrap();
            if g.authenticated {
                let now_ms = g.platform.get_time_utc_ms()?;
                g.try_upload_batch(now_ms, &e)?;
            }
            Ok(())
        });

        let inner = Arc::clone(&self.inner);
        bus.subscribe(move |ev: &ConfigChanged| {
            inner.lock().unwrap().batch_interval_ms = ev.batch_interval_ms as i64;
            Ok(())
        });

        Ok(())
    }

    fn save(&self) -> CoreResult<StateType> {
        let g = self.inner.lock().unwrap();
        let mut state = g.state.clone();
        state.device_credentials = g.upload_api.credentials().cloned();
        Ok(serde_json::to_value(&state)?)
    }
}

fn can_capture(settings: &DeviceSettings) -> bool {
    settings.owner.is_some()
}

fn batch_recipients(settings: &DeviceSettings) -> CoreResult<Vec<BatchRecipient>> {
    use crate::error::CoreError;
    let owner = settings
        .owner
        .clone()
        .ok_or(CoreError::InvalidState("owner public key not available"))?;
    let mut recipients = Vec::with_capacity(1 + settings.partners.len());
    recipients.push(owner);
    recipients.extend(settings.partners.clone());
    Ok(recipients)
}
