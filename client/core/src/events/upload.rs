pub mod api;
mod batch;

use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use api::UploadApi;
use batch::BatchBuilder;

use super::{Event, MAX_BATCH_ITEMS_PER_UPLOAD, POST_LOGIN_PROOF_BATCH_COUNT, log_error};
use crate::api::ApiTransport;
use crate::crypto::CryptoEngine;
use crate::error::CoreResult;
use crate::model::{BatchLogEntry, BatchRecipient, DeviceSettings, LogEntry};

const MAX_HASH_RETRIES_PER_LOOP: usize = 8;
const MAX_DIRECT_LOG_RETRIES_PER_LOOP: usize = 8;

pub struct UploadConfig {
    pub batch_interval: Duration,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct UploadObserverState {
    pub pending_batch_events: Vec<BatchLogEntry>,
    pub pending_hash_events: Vec<BatchLogEntry>,
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

pub struct UploadObserver<A: ApiTransport + Clone> {
    pub state: UploadObserverState,
    pub upload_api: UploadApi<A>,
    pub config: UploadConfig,
}

impl<A: ApiTransport + Clone> UploadObserver<A> {
    pub fn new(state: UploadObserverState, api: A, config: UploadConfig) -> Self {
        Self {
            state,
            upload_api: UploadApi::new(api),
            config,
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

    pub(super) fn on_event(&mut self, event: &Event, now_ms: i64) -> CoreResult<Vec<Event>> {
        match event {
            Event::ScreenshotCaptured { data } => self.handle_screenshot_captured(data, now_ms),
            Event::BatchUpload { data } => {
                self.state.pending_batch_events.push(data.clone());
                Ok(vec![])
            }
            Event::ImmediateUpload { entry } => {
                let keep = match self.try_upload_direct(entry) {
                    Ok(true) => false,
                    Ok(false) | Err(_) => true,
                };
                if keep {
                    self.state.pending_immediate_events.push(entry.clone());
                }
                Ok(vec![])
            }
            Event::Tick { now_ms } => {
                self.retry_pending_hashes()?;
                self.retry_pending_immediates()?;
                self.maybe_upload_batch(*now_ms)?;
                Ok(vec![])
            }
            Event::Shutdown => {
                if !self.state.pending_batch_events.is_empty() {
                    let _ = self.try_upload_batch(now_ms);
                }
                Ok(vec![])
            }
            Event::LifecycleObserved { .. } => Ok(vec![]),
        }
    }

    fn handle_screenshot_captured(
        &mut self,
        data: &BatchLogEntry,
        now_ms: i64,
    ) -> CoreResult<Vec<Event>> {
        let hash_base_url = self
            .state
            .settings
            .as_ref()
            .and_then(|s| s.hash_base_url.clone());
        match self.try_upload_hash(hash_base_url.as_deref(), data) {
            Ok(true) => self.state.pending_batch_events.push(data.clone()),
            Ok(false) | Err(_) => self.state.pending_hash_events.push(data.clone()),
        }
        let batch_interval_ms = self.config.batch_interval.as_millis() as i64;
        let can = self.state.settings.as_ref().map_or(false, can_capture);
        let should_upload = !self.state.pending_batch_events.is_empty()
            && can
            && (self.state.post_login_proof_batches_remaining > 0
                || self
                    .state
                    .last_batch_at_ms
                    .map(|last| now_ms - last >= batch_interval_ms)
                    .unwrap_or(true)
                || self.state.pending_batch_events.len() >= MAX_BATCH_ITEMS_PER_UPLOAD);
        if should_upload {
            let _ = self.try_upload_batch(now_ms);
        }
        Ok(vec![])
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
            match self.try_upload_hash(hash_base_url.as_deref(), &event) {
                Ok(true) => {
                    self.state.pending_batch_events.push(event);
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
        let can = self.state.settings.as_ref().map_or(false, can_capture);
        if self.state.pending_batch_events.is_empty() || !can {
            return Ok(());
        }
        let batch_interval_ms = self.config.batch_interval.as_millis() as i64;
        let should = self.state.post_login_proof_batches_remaining > 0
            || self
                .state
                .last_batch_at_ms
                .map(|last| now_ms - last >= batch_interval_ms)
                .unwrap_or(true);
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
        items.sort_by_key(|e| e.event.ts);
        let recipients = batch_recipients(settings)?;
        let batch = BatchBuilder::build_upload(&items, &CryptoEngine, &recipients, now_ms)?;
        match self.upload_api.upload_batch(&batch) {
            Ok(_) => {
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
        event: &BatchLogEntry,
    ) -> CoreResult<bool> {
        match self
            .upload_api
            .upload_hash(hash_base_url, &event.content_hash)
        {
            Ok(()) => Ok(true),
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
