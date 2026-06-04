pub mod api;
mod batch;

use std::any::Any;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use api::UploadApi;
use batch::BatchBuilder;
pub(crate) use batch::MAX_BATCH_ITEMS_PER_UPLOAD;

use crate::api::ApiTransport;
use crate::crypto::{CryptoEngine, compute_event_hash, encode_batch_event};
use crate::error::CoreResult;
use crate::events::log_error;
use crate::events::{Event, Observer, ProcessStoppedReason, StateType};
use crate::model::{BatchRecipient, DeviceCredentials, DeviceSettings, LogEntry};
use crate::platform::PlatformHooks;

pub(crate) const POST_LOGIN_PROOF_BATCH_COUNT: u32 = 3;

const MAX_HASH_RETRIES_PER_LOOP: usize = 8;
const MAX_DIRECT_LOG_RETRIES_PER_LOOP: usize = 8;

pub struct UploadConfig {
    pub batch_interval: Duration,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct UploadObserverState {
    pub pending_batch_events: Vec<(i64, Vec<u8>)>,
    pub pending_hash_events: Vec<LogEntry>,
    pub pending_immediate_events: Vec<LogEntry>,
    pub last_batch_at_ms: Option<i64>,
    pub post_login_proof_batches_remaining: u32,
    #[serde(default)]
    pub settings: Option<DeviceSettings>,
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
    }

    pub fn pending_request_count(&self) -> usize {
        self.pending_hash_events.len()
            + self.pending_immediate_events.len()
            + usize::from(!self.pending_batch_events.is_empty())
    }
}

pub struct UploadObserver<A: ApiTransport + Clone + 'static> {
    pub state: UploadObserverState,
    pub upload_api: UploadApi<A>,
    pub config: UploadConfig,
    platform: Box<dyn PlatformHooks>,
}

impl<A: ApiTransport + Clone + 'static> UploadObserver<A> {
    pub fn new(
        platform: Box<dyn PlatformHooks>,
        api: A,
        config: UploadConfig,
        initial_credentials: Option<DeviceCredentials>,
    ) -> Self {
        let mut upload_api = UploadApi::new(api);
        upload_api.set_credentials(initial_credentials);
        Self {
            state: UploadObserverState::default(),
            upload_api,
            config,
            platform,
        }
    }

    pub fn set_settings(&mut self, settings: Option<DeviceSettings>) {
        self.state.settings = settings;
    }

    pub fn has_settings(&self) -> bool {
        self.state.settings.is_some()
    }

    pub fn force_upload_now(&mut self, now_ms: i64) -> CoreResult<()> {
        self.try_upload_batch(now_ms)
    }

    fn retry_pending_hashes(&mut self) -> CoreResult<()> {
        let events = std::mem::take(&mut self.state.pending_hash_events);
        let mut still_pending = Vec::new();
        let mut stop = false;
        let mut retried = 0;
        let hash_base_url = self
            .state
            .settings
            .as_ref()
            .and_then(|s| s.hash_base_url.clone());
        for event in events {
            if stop || retried >= MAX_HASH_RETRIES_PER_LOOP {
                still_pending.push(event);
                continue;
            }
            let encoded = match encode_batch_event(&event) {
                Ok(bytes) => bytes,
                Err(err) => {
                    log_error("encode_batch_event failed permanently", Some(&err));
                    retried += 1;
                    continue;
                }
            };
            let hash = compute_event_hash(&encoded);
            match self.try_upload_hash(hash_base_url.as_deref(), &hash) {
                Ok(true) => {
                    self.state.pending_batch_events.push((event.ts, encoded));
                    retried += 1;
                }
                Ok(false) | Err(_) => {
                    still_pending.push(event);
                    stop = true;
                    retried += 1;
                }
            }
        }
        self.state.pending_hash_events = still_pending;
        Ok(())
    }

    fn retry_pending_immediates(&mut self) -> CoreResult<()> {
        let events = std::mem::take(&mut self.state.pending_immediate_events);
        let mut still_pending = Vec::new();
        let mut stop = false;
        let mut retried = 0;
        for entry in events {
            if stop || retried >= MAX_DIRECT_LOG_RETRIES_PER_LOOP {
                still_pending.push(entry);
                continue;
            }
            match self.try_upload_direct(&entry) {
                Ok(true) => {
                    retried += 1;
                }
                Ok(false) | Err(_) => {
                    still_pending.push(entry);
                    stop = true;
                    retried += 1;
                }
            }
        }
        self.state.pending_immediate_events = still_pending;
        Ok(())
    }

    fn maybe_upload_batch(&mut self, now_ms: i64) -> CoreResult<()> {
        let can = self.state.settings.as_ref().is_some_and(can_capture);
        if self.state.pending_batch_events.is_empty() || !can {
            return Ok(());
        }
        let batch_interval_ms = self.config.batch_interval.as_millis() as i64;
        let should = self.state.post_login_proof_batches_remaining > 0
            || self
                .state
                .last_batch_at_ms
                .map(|last| now_ms - last >= batch_interval_ms)
                .unwrap_or(true)
            || self.state.pending_batch_events.len() >= MAX_BATCH_ITEMS_PER_UPLOAD;
        if should {
            self.try_upload_batch(now_ms)?;
        }
        Ok(())
    }

    pub fn try_upload_batch(&mut self, now_ms: i64) -> CoreResult<()> {
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
            Err(err) if err.is_not_found() => Err(err),
            Err(err) if err.is_bad_request() => {
                log_error("batch upload failed permanently", Some(&err));
                self.state.pending_batch_events.drain(..count);
                Ok(())
            }
            Err(err) => {
                log_error("batch upload deferred", Some(&err));
                Ok(())
            }
        }
    }

    fn try_upload_hash(
        &mut self,
        hash_base_url: Option<&str>,
        content_hash: &[u8; 32],
    ) -> CoreResult<bool> {
        match self.upload_api.upload_hash(hash_base_url, content_hash) {
            Ok(()) => {
                #[cfg(debug_assertions)]
                eprintln!("[upload] hash ok: {}", hex::encode(&content_hash[..8]));
                Ok(true)
            }
            Err(err) if err.is_bad_request() => {
                log_error("hash upload failed permanently", Some(&err));
                Ok(true)
            }
            Err(err) => {
                log_error("hash upload deferred", Some(&err));
                Ok(false)
            }
        }
    }

    fn try_upload_direct(&mut self, entry: &LogEntry) -> CoreResult<bool> {
        match self.upload_api.upload_log(entry) {
            Ok(_) => Ok(true),
            Err(err) if err.is_bad_request() => {
                log_error("direct log upload failed permanently", Some(&err));
                Ok(true)
            }
            Err(err) => {
                log_error("direct log upload deferred", Some(&err));
                Ok(false)
            }
        }
    }
}

impl<A: ApiTransport + Clone + 'static> Observer for UploadObserver<A> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "upload"
    }

    fn save_state(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.state)?)
    }

    fn load_state(&mut self, state: StateType) -> CoreResult<()> {
        self.state = serde_json::from_value(state)?;
        Ok(())
    }

    fn on_event(&mut self, event: &Event) -> CoreResult<()> {
        match event {
            Event::Ping => {
                let now_ms = self.platform.get_time_utc_ms()?;

                // Sanity check
                if let Some(last) = self.state.last_batch_at_ms {
                    // Somehow got a time in the future, reset the schedule
                    if now_ms < last {
                        self.state.last_batch_at_ms = None;
                    }
                }

                self.retry_pending_hashes()?;
                self.retry_pending_immediates()?;
                self.maybe_upload_batch(now_ms)?;
            }
            Event::ProcessStopped(ProcessStoppedReason::Shutdown) => {
                let now_ms = self.platform.get_time_utc_ms()?;
                if !self.state.pending_batch_events.is_empty() {
                    let _ = self.try_upload_batch(now_ms);
                }
            }
            Event::Upload { risk, kind } => {
                let now_ms = self.platform.get_time_utc_ms()?;
                let entry = LogEntry {
                    ts: now_ms,
                    risk: Some(*risk),
                    event: kind.clone(),
                };
                if *risk >= crate::module::lifecycle::HIGH_RISK_LIFECYCLE_ALERT {
                    self.state.pending_immediate_events.push(entry);
                    self.retry_pending_immediates()?;
                } else {
                    self.state.pending_hash_events.push(entry);
                    self.retry_pending_hashes()?;
                    self.maybe_upload_batch(now_ms)?;
                }
            }
            _ => {}
        }
        Ok(())
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
