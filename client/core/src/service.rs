use crate::api::ApiClient;
use crate::audit::{derive_state, generate_local_id};
use crate::batch::BatchBuilder;
use crate::config::Config;
use crate::crypto::{
    CryptoEngine, prepare_log_batch_event, prepare_screenshot_batch_event, prepare_screenshot_event,
};
use crate::error::{CoreError, CoreResult};
use crate::image_pipeline::ImagePipeline;
use crate::model::{
    AuditLogItem, AuditLogPayload, AuditRecord, AuditState, AuthState, BatchRecipient, BatchUpload,
    BufferedBatchEvent, DeviceCredentials, DeviceSettings, EventData, LogEntry, LoginStatus,
    LoopOutcome, Screenshot, ServiceStatus,
};
use crate::platform::PlatformHooks;
use crate::storage::FileStateStore;

const POST_LOGIN_PROOF_BATCH_COUNT: u32 = 3;
const MAX_HASH_RETRIES_PER_LOOP: usize = 8;
const MAX_DIRECT_LOG_RETRIES_PER_LOOP: usize = 8;
const MAX_BATCH_ITEMS_PER_UPLOAD: usize = 25;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum RetryAttemptOutcome {
    Uploaded,
    Deferred,
    NonRetryable,
    ResetLoggedOut,
}

pub struct MonitorService<P> {
    config: Config,
    platform: P,
    api: ApiClient,
    storage: FileStateStore,
    user_access_token: Option<String>,
    device_credentials: Option<DeviceCredentials>,
    post_login_proof_batches_remaining: u32,
    device_settings: Option<DeviceSettings>,
    status: ServiceStatus,
}

impl<P: PlatformHooks> MonitorService<P> {
    pub fn setup(mut config: Config, platform: P) -> CoreResult<Self> {
        config.refresh_from_runtime_file()?;
        let api = ApiClient::new(&config)?;
        let storage = FileStateStore::new(&config.state_dir)?;
        let auth_state = storage.load_auth_state()?;
        let device_settings = storage.load_device_settings()?;
        let audit_state =
            derive_state(&storage.load_audit_records_at(platform.get_time_utc_ms()?)?);

        let mut status = storage.load_status()?.unwrap_or(ServiceStatus {
            is_authenticated: auth_state.device_credentials.is_some(),
            is_running: true,
            device_id: auth_state
                .device_credentials
                .as_ref()
                .map(|device| device.device_id.clone()),
            last_loop_at_ms: None,
            last_screenshot_at_ms: None,
            last_batch_at_ms: None,
            pending_request_count: audit_state.pending_request_count,
        });
        status.is_running = true;
        status.is_authenticated = auth_state.device_credentials.is_some();
        status.device_id = auth_state
            .device_credentials
            .as_ref()
            .map(|device| device.device_id.clone());
        status.pending_request_count = audit_state.pending_request_count;

        let mut service = Self {
            config,
            platform,
            api,
            storage,
            user_access_token: auth_state.user_access_token,
            device_credentials: auth_state.device_credentials,
            post_login_proof_batches_remaining: auth_state.post_login_proof_batches_remaining,
            device_settings,
            status,
        };

        if service.device_credentials.is_some() {
            let _ = service.refresh_device_settings();
        }
        service.persist_state()?;
        Ok(service)
    }

    pub fn loop_iteration(&mut self) -> CoreResult<LoopOutcome> {
        self.ensure_running()?;
        self.refresh_runtime_config()?;
        self.reload_persisted_state()?;

        let now_ms = self.platform.get_time_utc_ms()?;
        self.status.last_loop_at_ms = Some(now_ms);

        let work_result = (|| -> CoreResult<()> {
            if self.device_credentials.is_some() {
                self.retry_pending_work()?;
            }

            if self.can_capture() && self.should_take_screenshot(now_ms) {
                let screenshot = self.platform.take_screenshot()?;
                let processed = self.process_screenshot(screenshot)?;
                let item = self.enqueue_batch_event(processed, true)?;
                let _ = self.try_upload_hash_for_item(&item);
                self.status.last_screenshot_at_ms = Some(now_ms);
            }

            let audit_state = self.load_audit_state()?;
            if self.can_upload_batch(&audit_state) && self.should_upload_batch(now_ms) {
                self.refresh_device_settings()?;
                let batch_items = self.batch_upload_candidates(&audit_state);
                self.try_upload_pending_batch(batch_items, now_ms)?;
            }

            Ok(())
        })();

        self.persist_state()?;
        work_result?;

        Ok(LoopOutcome {
            ran_at_ms: now_ms,
            next_run_at_ms: self.next_run_at_ms(now_ms),
            status: self.status.clone(),
        })
    }

    pub fn shutdown(&mut self) -> CoreResult<()> {
        if !self.status.is_running {
            return Ok(());
        }

        let now_ms = self.platform.get_time_utc_ms()?;
        let _ = self.send_log(LogEntry {
            ts: now_ms,
            kind: "service_stop".to_string(),
            risk: None,
            data: EventData::from_pairs([("event".to_string(), "shutdown".to_string())]),
        });

        self.status.is_running = false;
        self.persist_state()
    }

    pub fn send_log(&mut self, log: LogEntry) -> CoreResult<()> {
        self.ensure_running()?;
        let item = self.append_audit_log(false, false, AuditLogPayload::for_direct_log(log))?;
        let _ = self.try_upload_direct_log(&item);
        self.persist_state()
    }

    pub fn queue_batch_log(
        &mut self,
        kind: &str,
        risk: Option<f32>,
        data: EventData,
    ) -> CoreResult<()> {
        self.ensure_running()?;
        let event = prepare_log_batch_event(self.platform.get_time_utc_ms()?, kind, risk, data)?;
        self.enqueue_batch_event(event, false)?;
        self.persist_state()
    }

    pub fn capture_batch_screenshot(
        &mut self,
        kind: &str,
        risk: Option<f32>,
        data: EventData,
    ) -> CoreResult<()> {
        self.ensure_running()?;
        let screenshot = self.platform.take_screenshot()?;
        let item = self.process_screenshot_with_data(screenshot, kind, risk, data)?;
        let item = self.enqueue_batch_event(item, true)?;
        let _ = self.try_upload_hash_for_item(&item);
        self.status.last_screenshot_at_ms = Some(self.platform.get_time_utc_ms()?);
        self.persist_state()
    }

    pub fn upload_pending_batch_now(&mut self) -> CoreResult<(usize, usize)> {
        self.ensure_running()?;
        self.refresh_runtime_config()?;
        self.reload_persisted_state()?;

        let audit_state = self.load_audit_state()?;
        let count = audit_state
            .pending_batch_uploads
            .len()
            .min(MAX_BATCH_ITEMS_PER_UPLOAD);
        if count == 0 {
            self.persist_state()?;
            return Ok((0, 0));
        }

        self.refresh_device_settings()?;
        let now_ms = self.platform.get_time_utc_ms()?;
        let batch_items = self.batch_upload_candidates(&audit_state);
        self.try_upload_pending_batch(batch_items, now_ms)?;
        let remaining = self.load_audit_state()?.pending_batch_uploads.len();
        self.persist_state()?;
        Ok((count, remaining))
    }

    pub fn login(&mut self, username: &str, password: &str) -> CoreResult<LoginStatus> {
        self.ensure_running()?;

        let access_token = self.api.login(username, password)?;
        let device = self.api.register_device(
            &access_token,
            &self.config.device_name,
            &self.config.platform_name,
        )?;

        self.storage.clear_audit_records()?;

        self.user_access_token = Some(access_token.clone());
        self.device_credentials = Some(device.clone());
        self.post_login_proof_batches_remaining = POST_LOGIN_PROOF_BATCH_COUNT;
        self.status.last_screenshot_at_ms = None;
        self.status.last_batch_at_ms = None;
        self.status.is_authenticated = true;
        self.status.device_id = Some(device.device_id.clone());
        self.persist_auth_state()?;

        self.refresh_device_settings()?;
        self.persist_state()?;

        let _ = self.send_log(LogEntry {
            ts: self.platform.get_time_utc_ms()?,
            kind: "system_event".to_string(),
            risk: None,
            data: EventData::from_pairs([
                ("event".to_string(), "login".to_string()),
                ("user".to_string(), username.to_string()),
            ]),
        });

        Ok(LoginStatus {
            access_token,
            device: Some(device),
        })
    }

    pub fn logout(&mut self) -> CoreResult<()> {
        self.ensure_running()?;

        if self.device_credentials.is_some() {
            let _ = self.send_log(LogEntry {
                ts: self.platform.get_time_utc_ms()?,
                kind: "system_event".to_string(),
                risk: None,
                data: EventData::from_pairs([("event".to_string(), "logout".to_string())]),
            });
        }

        if let Some(token) = self.user_access_token.as_deref() {
            let _ = self.api.logout(token);
        }

        self.user_access_token = None;
        self.device_credentials = None;
        self.post_login_proof_batches_remaining = 0;
        self.device_settings = None;
        self.storage.clear_audit_records()?;
        self.status.is_authenticated = false;
        self.status.device_id = None;
        self.persist_state()
    }

    pub fn status(&self) -> CoreResult<ServiceStatus> {
        let mut status = self
            .storage
            .load_status()?
            .unwrap_or_else(|| self.status.clone());
        status.pending_request_count = self.load_audit_state()?.pending_request_count;
        Ok(status)
    }

    fn process_screenshot(&self, screenshot: Screenshot) -> CoreResult<BufferedBatchEvent> {
        let processed = ImagePipeline.process(screenshot)?;
        prepare_screenshot_event(processed)
    }

    fn process_screenshot_with_data(
        &self,
        screenshot: Screenshot,
        kind: &str,
        risk: Option<f32>,
        data: EventData,
    ) -> CoreResult<BufferedBatchEvent> {
        let processed = ImagePipeline.process(screenshot)?;
        prepare_screenshot_batch_event(processed, kind, risk, data)
    }

    fn enqueue_batch_event(
        &mut self,
        event: BufferedBatchEvent,
        requires_hash_upload: bool,
    ) -> CoreResult<AuditLogItem> {
        self.append_audit_log(
            true,
            requires_hash_upload,
            AuditLogPayload::for_batch_event(event),
        )
    }

    fn append_audit_log(
        &mut self,
        should_be_in_batch: bool,
        requires_hash_upload: bool,
        payload: AuditLogPayload,
    ) -> CoreResult<AuditLogItem> {
        let local_id = generate_local_id();
        let record = AuditRecord::Log {
            local_id: local_id.clone(),
            should_be_in_batch,
            requires_hash_upload,
            log: payload.clone(),
        };
        let audit_day = self.storage.append_audit_log_record(&record)?;
        Ok(AuditLogItem {
            audit_day,
            local_id,
            should_be_in_batch,
            requires_hash_upload,
            payload,
        })
    }

    fn load_audit_state(&self) -> CoreResult<AuditState> {
        Ok(derive_state(
            &self
                .storage
                .load_audit_records_at(self.platform.get_time_utc_ms()?)?,
        ))
    }

    fn try_upload_hash_for_item(&mut self, item: &AuditLogItem) -> CoreResult<RetryAttemptOutcome> {
        let Some(batch_event) = item.payload.as_batch_event() else {
            self.log_error(
                "hash upload skipped; batch payload missing",
                Some(&item.local_id),
                None,
            );
            return Ok(RetryAttemptOutcome::NonRetryable);
        };

        let hash_base_url = self
            .device_settings
            .as_ref()
            .and_then(|settings| settings.hash_base_url.clone());
        match self.with_device_token_retry(|api, access_token, _| {
            api.upload_hash(
                hash_base_url.as_deref(),
                access_token,
                &batch_event.content_hash,
            )
        }) {
            Ok(()) => {
                self.storage.append_audit_record_for_day(
                    &item.audit_day,
                    &AuditRecord::HashUploaded {
                        local_id: item.local_id.clone(),
                    },
                )?;
                Ok(RetryAttemptOutcome::Uploaded)
            }
            Err(err) if err.is_not_found() => {
                self.reset_local_state_after_not_found(Some(&item.local_id), &err)?;
                Ok(RetryAttemptOutcome::ResetLoggedOut)
            }
            Err(err) if err.is_bad_request() => {
                self.log_error(
                    "hash upload failed permanently",
                    Some(&item.local_id),
                    Some(&err),
                );
                Ok(RetryAttemptOutcome::NonRetryable)
            }
            Err(err) => {
                self.log_error("hash upload deferred", Some(&item.local_id), Some(&err));
                Ok(RetryAttemptOutcome::Deferred)
            }
        }
    }

    fn try_upload_direct_log(&mut self, item: &AuditLogItem) -> CoreResult<RetryAttemptOutcome> {
        let Some(log) = item.payload.as_direct_log() else {
            self.log_error(
                "direct log upload skipped; direct payload missing",
                Some(&item.local_id),
                None,
            );
            return Ok(RetryAttemptOutcome::NonRetryable);
        };

        match self.with_device_token_retry(|api, access_token, _| api.upload_log(access_token, log))
        {
            Ok(response) => {
                self.storage.append_audit_record_for_day(
                    &item.audit_day,
                    &AuditRecord::LogUploaded {
                        local_id: item.local_id.clone(),
                        server_id: Some(response.id),
                        batch_id: None,
                    },
                )?;
                Ok(RetryAttemptOutcome::Uploaded)
            }
            Err(err) if err.is_not_found() => {
                self.reset_local_state_after_not_found(Some(&item.local_id), &err)?;
                Ok(RetryAttemptOutcome::ResetLoggedOut)
            }
            Err(err) if err.is_bad_request() => {
                self.log_error(
                    "direct log upload failed permanently",
                    Some(&item.local_id),
                    Some(&err),
                );
                Ok(RetryAttemptOutcome::NonRetryable)
            }
            Err(err) => {
                self.log_error(
                    "direct log upload deferred",
                    Some(&item.local_id),
                    Some(&err),
                );
                Ok(RetryAttemptOutcome::Deferred)
            }
        }
    }

    fn try_upload_pending_batch(&mut self, items: &[AuditLogItem], now_ms: i64) -> CoreResult<()> {
        let batch_events = items
            .iter()
            .filter_map(|item| {
                if item.payload.as_batch_event().is_none() {
                    self.log_error(
                        "batch upload skipped item; batch payload missing",
                        Some(&item.local_id),
                        None,
                    );
                }
                item.payload.as_batch_event().cloned()
            })
            .collect::<Vec<_>>();
        if batch_events.is_empty() {
            return Ok(());
        }

        let mut batch_events = batch_events;
        batch_events.sort_by_key(|item| item.event.ts);
        let batch = self.build_batch(&batch_events, now_ms)?;

        match self
            .with_device_token_retry(|api, access_token, _| api.upload_batch(access_token, &batch))
        {
            Ok(response) => {
                let batch_day = items
                    .first()
                    .map(|item| item.audit_day.as_str())
                    .unwrap_or("1970-01-01");
                self.storage.append_audit_record_for_day(
                    batch_day,
                    &AuditRecord::BatchUploaded {
                        server_id: response.id.clone(),
                    },
                )?;
                for item in items {
                    if item.payload.as_batch_event().is_none() {
                        continue;
                    }
                    self.storage.append_audit_record_for_day(
                        &item.audit_day,
                        &AuditRecord::LogUploaded {
                            local_id: item.local_id.clone(),
                            server_id: None,
                            batch_id: Some(response.id.clone()),
                        },
                    )?;
                }
                self.complete_batch_upload(now_ms);
                Ok(())
            }
            Err(err) if err.is_not_found() => {
                self.reset_local_state_after_not_found(Some("batch-upload"), &err)?;
                Ok(())
            }
            Err(err) if err.is_bad_request() => {
                self.log_error("batch upload failed permanently", None, Some(&err));
                Ok(())
            }
            Err(err) => {
                self.log_error("batch upload deferred", None, Some(&err));
                Ok(())
            }
        }
    }

    fn build_batch(
        &self,
        batch_events: &[BufferedBatchEvent],
        now_ms: i64,
    ) -> CoreResult<BatchUpload> {
        let recipients = self.batch_recipients()?;
        BatchBuilder::build_upload(batch_events, &CryptoEngine, &recipients, now_ms)
    }

    fn retry_pending_work(&mut self) -> CoreResult<()> {
        let audit_state = self.load_audit_state()?;
        for item in audit_state
            .pending_hash_uploads
            .iter()
            .take(MAX_HASH_RETRIES_PER_LOOP)
        {
            if matches!(
                self.try_upload_hash_for_item(item)?,
                RetryAttemptOutcome::Deferred | RetryAttemptOutcome::ResetLoggedOut
            ) {
                break;
            }
        }

        if self.device_credentials.is_none() {
            return Ok(());
        }

        let audit_state = self.load_audit_state()?;
        for item in audit_state
            .pending_direct_uploads
            .iter()
            .take(MAX_DIRECT_LOG_RETRIES_PER_LOOP)
        {
            if matches!(
                self.try_upload_direct_log(item)?,
                RetryAttemptOutcome::Deferred | RetryAttemptOutcome::ResetLoggedOut
            ) {
                break;
            }
        }

        Ok(())
    }

    fn refresh_device_settings(&mut self) -> CoreResult<()> {
        match self
            .with_device_token_retry(|api, access_token, _| api.get_device_settings(access_token))
        {
            Ok(settings) => {
                self.device_settings = Some(settings);
                self.storage
                    .save_device_settings(self.device_settings.as_ref())?;
                self.status.is_authenticated = self.device_credentials.is_some();
                Ok(())
            }
            Err(err) if err.is_not_found() => {
                self.reset_local_state_after_not_found(Some("device-settings"), &err)?;
                Err(CoreError::NotAuthenticated)
            }
            Err(err) => {
                self.log_error(
                    "device settings refresh failed",
                    Some("device-settings"),
                    Some(&err),
                );
                Err(err)
            }
        }
    }

    fn persist_state(&mut self) -> CoreResult<()> {
        self.status.is_authenticated = self.device_credentials.is_some();
        self.status.device_id = self
            .device_credentials
            .as_ref()
            .map(|credentials| credentials.device_id.clone());
        self.status.pending_request_count = self.load_audit_state()?.pending_request_count;

        self.storage.save_status(&self.status)?;
        self.storage
            .save_device_settings(self.device_settings.as_ref())?;
        self.persist_auth_state()
    }

    fn persist_auth_state(&self) -> CoreResult<()> {
        self.storage.save_auth_state(&AuthState {
            user_access_token: self.user_access_token.clone(),
            device_credentials: self.device_credentials.clone(),
            post_login_proof_batches_remaining: self.post_login_proof_batches_remaining,
        })
    }

    fn reset_local_state_after_not_found(
        &mut self,
        request_id: Option<&str>,
        error: &CoreError,
    ) -> CoreResult<()> {
        self.log_error(
            "remote state missing; clearing local auth and audit state",
            request_id,
            Some(error),
        );
        self.user_access_token = None;
        self.device_credentials = None;
        self.post_login_proof_batches_remaining = 0;
        self.device_settings = None;
        self.storage.clear_audit_records()?;
        self.status.is_authenticated = false;
        self.status.device_id = None;
        self.status.last_screenshot_at_ms = None;
        self.status.last_batch_at_ms = None;
        self.persist_state()
    }

    fn reload_persisted_state(&mut self) -> CoreResult<()> {
        let auth_state = self.storage.load_auth_state()?;
        let previous_post_login_proof_batches_remaining = self.post_login_proof_batches_remaining;

        self.user_access_token = auth_state.user_access_token;
        self.device_credentials = auth_state.device_credentials;
        self.post_login_proof_batches_remaining = auth_state.post_login_proof_batches_remaining;
        self.device_settings = self.storage.load_device_settings()?;

        let proof_burst_started = previous_post_login_proof_batches_remaining == 0
            && self.post_login_proof_batches_remaining > 0;
        if proof_burst_started {
            self.status.last_screenshot_at_ms = None;
            self.status.last_batch_at_ms = None;
        }
        Ok(())
    }

    fn refresh_runtime_config(&mut self) -> CoreResult<()> {
        let previous_base_url = self.config.api_base_url.clone();
        self.config.refresh_from_runtime_file()?;
        if self.config.api_base_url != previous_base_url {
            self.api = ApiClient::new(&self.config)?;
        }
        Ok(())
    }

    fn ensure_running(&self) -> CoreResult<()> {
        if self.status.is_running {
            Ok(())
        } else {
            Err(CoreError::Shutdown)
        }
    }

    fn can_capture(&self) -> bool {
        self.device_credentials.is_some()
            && self
                .device_settings
                .as_ref()
                .map(|settings| settings.enabled && settings.owner.is_some())
                .unwrap_or(false)
    }

    fn can_upload_batch(&self, audit_state: &AuditState) -> bool {
        self.can_capture() && !audit_state.pending_batch_uploads.is_empty()
    }

    fn batch_upload_candidates<'a>(&self, audit_state: &'a AuditState) -> &'a [AuditLogItem] {
        let count = audit_state
            .pending_batch_uploads
            .len()
            .min(MAX_BATCH_ITEMS_PER_UPLOAD);
        &audit_state.pending_batch_uploads[..count]
    }

    fn should_take_screenshot(&self, now_ms: i64) -> bool {
        match self.status.last_screenshot_at_ms {
            Some(last) => now_ms - last >= self.config.screenshot_interval.as_millis() as i64,
            None => true,
        }
    }

    fn should_upload_batch(&self, now_ms: i64) -> bool {
        if self.post_login_proof_batches_remaining > 0 {
            return true;
        }
        match self.status.last_batch_at_ms {
            Some(last) => now_ms - last >= self.config.batch_interval.as_millis() as i64,
            None => true,
        }
    }

    fn complete_batch_upload(&mut self, now_ms: i64) {
        if self.post_login_proof_batches_remaining > 0 {
            self.post_login_proof_batches_remaining -= 1;
        }
        self.status.last_batch_at_ms = Some(now_ms);
    }

    fn next_run_at_ms(&self, now_ms: i64) -> i64 {
        let screenshot_due = self.status.last_screenshot_at_ms.map_or(
            now_ms + self.config.screenshot_interval.as_millis() as i64,
            |last| last + self.config.screenshot_interval.as_millis() as i64,
        );
        let batch_due = self.status.last_batch_at_ms.map_or(
            now_ms + self.config.batch_interval.as_millis() as i64,
            |last| last + self.config.batch_interval.as_millis() as i64,
        );
        screenshot_due.min(batch_due)
    }

    fn batch_recipients(&self) -> CoreResult<Vec<BatchRecipient>> {
        let settings = self
            .device_settings
            .as_ref()
            .ok_or(CoreError::InvalidState("device settings not available"))?;
        let owner = settings
            .owner
            .clone()
            .ok_or(CoreError::InvalidState("owner public key not available"))?;

        let mut recipients = Vec::with_capacity(1 + settings.partners.len());
        recipients.push(owner);
        recipients.extend(settings.partners.clone());
        Ok(recipients)
    }

    fn with_device_token_retry<T, F>(&mut self, mut operation: F) -> CoreResult<T>
    where
        F: FnMut(&ApiClient, &str, Option<&str>) -> CoreResult<T>,
    {
        let credentials = self
            .device_credentials
            .as_ref()
            .ok_or(CoreError::NotAuthenticated)?
            .clone();
        let hash_base_url = self
            .device_settings
            .as_ref()
            .and_then(|settings| settings.hash_base_url.as_deref());

        match operation(&self.api, &credentials.access_token, hash_base_url) {
            Ok(value) => Ok(value),
            Err(err) if err.is_unauthorized() => {
                let refreshed = self.api.refresh_device_token(&credentials.refresh_token)?;
                if let Some(device_credentials) = self.device_credentials.as_mut() {
                    device_credentials.access_token = refreshed.clone();
                }
                self.persist_auth_state()?;
                operation(&self.api, &refreshed, hash_base_url)
            }
            Err(err) => Err(err),
        }
    }

    fn log_error(&self, message: &str, request_id: Option<&str>, error: Option<&CoreError>) {
        let ts = self
            .platform
            .get_time_utc_ms()
            .map(|value| value.to_string())
            .unwrap_or_else(|_| "unknown-ts".to_string());
        let request_id = request_id.unwrap_or("-");
        let error_text = error
            .map(ToString::to_string)
            .unwrap_or_else(|| "unknown error".to_string());
        let line = format!("[{ts}] {message}; request_id={request_id}; error={error_text}");
        let _ = self.storage.append_error_log(&line);
        eprintln!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use super::*;

    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[derive(Clone)]
    struct TestPlatform;

    impl PlatformHooks for TestPlatform {
        fn take_screenshot(&self) -> CoreResult<Screenshot> {
            Ok(Screenshot {
                captured_at_ms: 0,
                bytes: Vec::new(),
                content_type: "image/png".to_string(),
            })
        }

        fn get_time_utc_ms(&self) -> CoreResult<i64> {
            Ok(0)
        }
    }

    fn test_config(state_dir: PathBuf) -> Config {
        Config::new(
            "https://example.invalid",
            "test-device",
            "test-platform",
            state_dir,
            None,
            Duration::from_secs(300),
            Duration::from_secs(3600),
        )
    }

    fn temp_state_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "virtue-core-test-{}-{}",
            std::process::id(),
            TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temp state dir");
        path
    }

    fn build_service(state_dir: PathBuf) -> MonitorService<TestPlatform> {
        let config = test_config(state_dir.clone());
        let storage = FileStateStore::new(&state_dir).expect("create file state store");
        MonitorService {
            api: ApiClient::new(&config).expect("create api client"),
            config,
            platform: TestPlatform,
            storage,
            user_access_token: None,
            device_credentials: None,
            post_login_proof_batches_remaining: 0,
            device_settings: Some(DeviceSettings {
                device_id: "device-1".to_string(),
                name: "Device".to_string(),
                platform: "test".to_string(),
                enabled: true,
                owner: Some(BatchRecipient {
                    user_id: "user-1".to_string(),
                    pub_key_base64: "owner-key".to_string(),
                }),
                partners: Vec::new(),
                hash_base_url: None,
            }),
            status: ServiceStatus {
                is_authenticated: false,
                is_running: true,
                device_id: None,
                last_loop_at_ms: None,
                last_screenshot_at_ms: Some(1000),
                last_batch_at_ms: Some(1000),
                pending_request_count: 0,
            },
        }
    }

    #[test]
    fn post_login_proof_uploads_ignore_batch_interval() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());
        service.post_login_proof_batches_remaining = 2;

        assert!(service.should_upload_batch(1001));

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn reloading_new_login_state_resets_capture_schedule() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());
        service.post_login_proof_batches_remaining = 0;
        service.device_credentials = None;

        service
            .storage
            .save_auth_state(&AuthState {
                user_access_token: Some("user-token".to_string()),
                device_credentials: Some(DeviceCredentials {
                    device_id: "device-2".to_string(),
                    access_token: "device-access".to_string(),
                    refresh_token: "device-refresh".to_string(),
                }),
                post_login_proof_batches_remaining: POST_LOGIN_PROOF_BATCH_COUNT,
            })
            .expect("persist auth state");

        service.reload_persisted_state().expect("reload state");

        assert_eq!(service.status.last_screenshot_at_ms, None);
        assert_eq!(service.status.last_batch_at_ms, None);
        assert_eq!(
            service.post_login_proof_batches_remaining,
            POST_LOGIN_PROOF_BATCH_COUNT
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn complete_batch_upload_consumes_one_proof_batch() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());
        service.post_login_proof_batches_remaining = 2;

        service.complete_batch_upload(1001);
        assert_eq!(service.post_login_proof_batches_remaining, 1);

        service.complete_batch_upload(1002);
        service.complete_batch_upload(1003);
        assert_eq!(service.post_login_proof_batches_remaining, 0);
        assert_eq!(service.status.last_batch_at_ms, Some(1003));

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn status_derives_pending_request_count_from_audit_log() {
        let state_dir = temp_state_dir();
        let service = build_service(state_dir.clone());
        service
            .storage
            .save_status(&ServiceStatus {
                is_authenticated: true,
                is_running: true,
                device_id: Some("device-1".to_string()),
                last_loop_at_ms: Some(1),
                last_screenshot_at_ms: Some(1),
                last_batch_at_ms: Some(1),
                pending_request_count: 0,
            })
            .expect("save stale status");
        service
            .storage
            .append_audit_record(&AuditRecord::Log {
                local_id: "pending-log".to_string(),
                should_be_in_batch: false,
                requires_hash_upload: false,
                log: AuditLogPayload::for_direct_log(LogEntry {
                    ts: 1,
                    kind: "system_event".to_string(),
                    risk: None,
                    data: EventData::from_pairs([("event".to_string(), "test".to_string())]),
                }),
            })
            .expect("append audit record");

        let status = service.status().expect("load status");

        assert_eq!(status.pending_request_count, 1);

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn batch_upload_candidates_are_capped() {
        let state_dir = temp_state_dir();
        let service = build_service(state_dir.clone());
        let audit_state = AuditState {
            pending_batch_uploads: (0..(MAX_BATCH_ITEMS_PER_UPLOAD + 5))
                .map(|index| AuditLogItem {
                    audit_day: "1970-01-01".to_string(),
                    local_id: format!("batch-{index}"),
                    should_be_in_batch: true,
                    requires_hash_upload: false,
                    payload: AuditLogPayload::for_batch_event(BufferedBatchEvent {
                        event: crate::model::BatchEvent {
                            ts: index as i64,
                            kind: "screenshot".to_string(),
                            risk: None,
                            data: crate::model::BatchEventData::from_pairs([])
                                .with_screenshot(Vec::new(), "image/png"),
                        },
                        content_hash: [0; 32],
                    }),
                })
                .collect(),
            ..AuditState::default()
        };

        let candidates = service.batch_upload_candidates(&audit_state);

        assert_eq!(candidates.len(), MAX_BATCH_ITEMS_PER_UPLOAD);
        assert_eq!(candidates[0].local_id, "batch-0");
        assert_eq!(
            candidates[MAX_BATCH_ITEMS_PER_UPLOAD - 1].local_id,
            format!("batch-{}", MAX_BATCH_ITEMS_PER_UPLOAD - 1)
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn queue_batch_log_creates_pending_batch_item_without_hash_upload() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());

        service
            .queue_batch_log(
                "developer_log",
                Some(0.7),
                EventData::from_pairs([
                    ("source".to_string(), "test".to_string()),
                    ("title".to_string(), "Developer test".to_string()),
                ]),
            )
            .expect("queue batch log");

        let audit_state = service.load_audit_state().expect("load audit state");

        assert_eq!(audit_state.pending_hash_uploads.len(), 0);
        assert_eq!(audit_state.pending_batch_uploads.len(), 1);
        let queued = &audit_state.pending_batch_uploads[0];
        let batch_event = queued.payload.as_batch_event().expect("queued batch event");
        assert_eq!(batch_event.event.kind, "developer_log");
        assert_eq!(batch_event.event.risk, Some(0.7));
        assert_eq!(
            batch_event.event.data,
            EventData::from_pairs([
                ("source".to_string(), "test".to_string()),
                ("title".to_string(), "Developer test".to_string()),
            ])
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn reset_local_state_after_not_found_clears_auth_and_audit_log() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());
        service.user_access_token = Some("user-token".to_string());
        service.device_credentials = Some(DeviceCredentials {
            device_id: "device-1".to_string(),
            access_token: "device-access".to_string(),
            refresh_token: "device-refresh".to_string(),
        });
        service.status.is_authenticated = true;
        service.status.device_id = Some("device-1".to_string());
        service
            .storage
            .append_audit_record(&AuditRecord::Log {
                local_id: "pending-log".to_string(),
                should_be_in_batch: false,
                requires_hash_upload: false,
                log: AuditLogPayload::for_direct_log(LogEntry {
                    ts: 1,
                    kind: "system_event".to_string(),
                    risk: None,
                    data: EventData::from_pairs([("event".to_string(), "test".to_string())]),
                }),
            })
            .expect("append audit record");

        service
            .reset_local_state_after_not_found(
                Some("pending-log"),
                &CoreError::HttpStatus {
                    status: 404,
                    message: "Not found".to_string(),
                },
            )
            .expect("reset local state");

        let auth_state = service.storage.load_auth_state().expect("load auth");
        let status = service.status().expect("load status");

        assert!(auth_state.user_access_token.is_none());
        assert!(auth_state.device_credentials.is_none());
        assert_eq!(
            service
                .storage
                .load_audit_records()
                .expect("load audit")
                .len(),
            0
        );
        assert!(!status.is_authenticated);
        assert_eq!(status.device_id, None);
        assert_eq!(status.pending_request_count, 0);

        let _ = fs::remove_dir_all(state_dir);
    }
}
