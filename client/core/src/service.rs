use crate::api::ApiClient;
use crate::batch::BatchBuilder;
use crate::config::Config;
use crate::crypto::{CryptoEngine, prepare_screenshot_event};
use crate::error::{CoreError, CoreResult};
use crate::image_pipeline::ImagePipeline;
use crate::model::{
    AuthState, BatchBufferState, BatchRecipient, BatchUpload, BufferedScreenshot, DeviceCredentials,
    DeviceSettings, LogEntry, LoginStatus, LoopOutcome, PendingRequest, RequestDisposition,
    RequestKind, Screenshot, ServiceStatus,
};
use crate::platform::PlatformHooks;
use crate::storage::FileStateStore;

pub struct MonitorService<P> {
    config: Config,
    platform: P,
    api: ApiClient,
    storage: FileStateStore,
    user_access_token: Option<String>,
    device_credentials: Option<DeviceCredentials>,
    device_settings: Option<DeviceSettings>,
    batch_buffer: BatchBufferState,
    pending_requests: Vec<PendingRequest>,
    status: ServiceStatus,
}

impl<P: PlatformHooks> MonitorService<P> {
    pub fn setup(mut config: Config, platform: P) -> CoreResult<Self> {
        config.refresh_from_runtime_file()?;
        let api = ApiClient::new(&config)?;
        let storage = FileStateStore::new(&config.state_dir)?;
        let auth_state = storage.load_auth_state()?;
        let pending_requests = storage.load_pending_requests()?;
        let batch_buffer = storage.load_batch_buffer()?;
        let device_settings = storage.load_device_settings()?;

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
            pending_request_count: pending_requests.len(),
        });
        status.is_running = true;
        status.is_authenticated = auth_state.device_credentials.is_some();
        status.device_id = auth_state
            .device_credentials
            .as_ref()
            .map(|device| device.device_id.clone());
        status.pending_request_count = pending_requests.len();

        let mut service = Self {
            config,
            platform,
            api,
            storage,
            user_access_token: auth_state.user_access_token,
            device_credentials: auth_state.device_credentials,
            device_settings,
            batch_buffer,
            pending_requests,
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

        if self.device_credentials.is_some() {
            self.retry_failed_requests()?;
        }

        if self.can_capture() && self.should_take_screenshot(now_ms) {
            let screenshot = self.platform.take_screenshot()?;
            let processed = self.process_screenshot(screenshot)?;
            self.store_screenshot(&processed)?;
            let _ = self.try_request(self.pending_hash_request(&processed, now_ms)?)?;
            self.status.last_screenshot_at_ms = Some(now_ms);
        }

        if self.can_upload_batch() && self.should_upload_batch(now_ms) {
            self.refresh_device_settings()?;
            let batch = self.build_batch(now_ms)?;
            let _ = self.try_request(self.pending_batch_request(&batch, now_ms)?)?;
            BatchBuilder::clear(&mut self.batch_buffer);
            self.status.last_batch_at_ms = Some(now_ms);
        }

        self.persist_state()?;

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
            ts_ms: now_ms,
            kind: "service_stop".to_string(),
            risk: None,
            data: serde_json::json!({
                "event": "shutdown",
            }),
        });

        self.status.is_running = false;
        self.persist_state()
    }

    pub fn send_log(&mut self, log: LogEntry) -> CoreResult<()> {
        self.ensure_running()?;
        let request = PendingRequest {
            id: format!("log-{}", log.ts_ms),
            kind: RequestKind::UploadLog,
            payload: serde_json::to_value(log)?,
            last_tried_at_ms: None,
            try_count: 0,
        };
        let _ = self.try_request(request)?;
        self.persist_state()
    }

    pub fn login(&mut self, username: &str, password: &str) -> CoreResult<LoginStatus> {
        self.ensure_running()?;

        let access_token = self.api.login(username, password)?;
        let device = self.api.register_device(
            &access_token,
            &self.config.device_name,
            &self.config.platform_name,
        )?;

        self.user_access_token = Some(access_token.clone());
        self.device_credentials = Some(device.clone());
        self.status.is_authenticated = true;
        self.status.device_id = Some(device.device_id.clone());
        self.persist_auth_state()?;

        self.refresh_device_settings()?;
        self.persist_state()?;

        let _ = self.send_log(LogEntry {
            ts_ms: self.platform.get_time_utc_ms()?,
            kind: "system_event".to_string(),
            risk: None,
            data: serde_json::json!({
                "event": "login",
                "user": username,
            }),
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
                ts_ms: self.platform.get_time_utc_ms()?,
                kind: "system_event".to_string(),
                risk: None,
                data: serde_json::json!({
                    "event": "logout",
                }),
            });
        }

        if let Some(token) = self.user_access_token.as_deref() {
            let _ = self.api.logout(token);
        }

        self.user_access_token = None;
        self.device_credentials = None;
        self.device_settings = None;
        self.batch_buffer = BatchBufferState::default();
        self.pending_requests.clear();
        self.status.is_authenticated = false;
        self.status.device_id = None;
        self.persist_state()
    }

    pub fn status(&self) -> CoreResult<ServiceStatus> {
        Ok(self
            .storage
            .load_status()?
            .unwrap_or_else(|| self.status.clone()))
    }

    fn process_screenshot(&self, screenshot: Screenshot) -> CoreResult<BufferedScreenshot> {
        let processed = ImagePipeline.process(screenshot)?;
        Ok(prepare_screenshot_event(processed))
    }

    fn store_screenshot(&mut self, processed: &BufferedScreenshot) -> CoreResult<()> {
        BatchBuilder::push_screenshot(&mut self.batch_buffer, processed.clone())?;
        self.storage.save_batch_buffer(&self.batch_buffer)
    }

    fn build_batch(&self, now_ms: i64) -> CoreResult<BatchUpload> {
        let recipients = self.batch_recipients()?;
        BatchBuilder::build_upload(&self.batch_buffer, &CryptoEngine, &recipients, now_ms)
    }

    fn pending_hash_request(
        &self,
        processed: &BufferedScreenshot,
        now_ms: i64,
    ) -> CoreResult<PendingRequest> {
        Ok(PendingRequest {
            id: format!("hash-{now_ms}"),
            kind: RequestKind::UploadHash,
            payload: serde_json::json!({
                "content_hash": processed.content_hash,
            }),
            last_tried_at_ms: None,
            try_count: 0,
        })
    }

    fn pending_batch_request(
        &self,
        batch: &BatchUpload,
        now_ms: i64,
    ) -> CoreResult<PendingRequest> {
        Ok(PendingRequest {
            id: format!("batch-{now_ms}"),
            kind: RequestKind::UploadBatch,
            payload: serde_json::to_value(batch)?,
            last_tried_at_ms: None,
            try_count: 0,
        })
    }

    fn try_request(&mut self, mut request: PendingRequest) -> CoreResult<RequestDisposition> {
        match self.execute_request(&request) {
            Ok(()) => Ok(RequestDisposition::Completed),
            Err(err) if err.is_bad_request() => {
                self.log_error(
                    "dropping bad request without retry",
                    Some(&request),
                    Some(&err),
                );
                Ok(RequestDisposition::Completed)
            }
            Err(err) => {
                self.log_error(
                    "request failed, queuing for retry",
                    Some(&request),
                    Some(&err),
                );
                request.try_count += 1;
                request.last_tried_at_ms = Some(self.platform.get_time_utc_ms()?);
                self.pending_requests.push(request);
                Ok(RequestDisposition::Deferred)
            }
        }
    }

    fn execute_request(&mut self, request: &PendingRequest) -> CoreResult<()> {
        match request.kind {
            RequestKind::DeviceSettings => {
                let settings = self.with_device_token_retry(|api, access_token, _| {
                    api.get_device_settings(access_token)
                })?;
                self.device_settings = Some(settings);
                self.storage
                    .save_device_settings(self.device_settings.as_ref())?;
                Ok(())
            }
            RequestKind::UploadBatch => {
                let batch: BatchUpload = serde_json::from_value(request.payload.clone())?;
                self.with_device_token_retry(|api, access_token, _| {
                    api.upload_batch(access_token, &batch)
                })
            }
            RequestKind::UploadLog => {
                let log: LogEntry = serde_json::from_value(request.payload.clone())?;
                self.with_device_token_retry(|api, access_token, _| {
                    api.upload_log(access_token, &log)
                })
            }
            RequestKind::UploadHash => {
                let content_hash = parse_content_hash(&request.payload)?;
                let hash_base_url = self
                    .device_settings
                    .as_ref()
                    .and_then(|settings| settings.hash_base_url.clone());
                self.with_device_token_retry(|api, access_token, _| {
                    api.upload_hash(hash_base_url.as_deref(), access_token, &content_hash)
                })
            }
        }
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

    fn refresh_device_settings(&mut self) -> CoreResult<()> {
        let request = PendingRequest {
            id: "device-settings".to_string(),
            kind: RequestKind::DeviceSettings,
            payload: serde_json::json!({}),
            last_tried_at_ms: None,
            try_count: 0,
        };

        match self.execute_request(&request) {
            Ok(()) => {
                self.status.is_authenticated = self.device_credentials.is_some();
                Ok(())
            }
            Err(err) => {
                self.log_error("device settings refresh failed", Some(&request), Some(&err));
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
        self.status.pending_request_count = self.pending_requests.len();

        self.storage.save_status(&self.status)?;
        self.storage.save_pending_requests(&self.pending_requests)?;
        self.storage.save_batch_buffer(&self.batch_buffer)?;
        self.storage
            .save_device_settings(self.device_settings.as_ref())?;
        self.persist_auth_state()
    }

    fn persist_auth_state(&self) -> CoreResult<()> {
        self.storage.save_auth_state(&AuthState {
            user_access_token: self.user_access_token.clone(),
            device_credentials: self.device_credentials.clone(),
        })
    }

    fn reload_persisted_state(&mut self) -> CoreResult<()> {
        let auth_state = self.storage.load_auth_state()?;
        self.user_access_token = auth_state.user_access_token;
        self.device_credentials = auth_state.device_credentials;

        self.device_settings = self.storage.load_device_settings()?;
        self.pending_requests = self.storage.load_pending_requests()?;
        self.batch_buffer = self.storage.load_batch_buffer()?;
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

    fn can_upload_batch(&self) -> bool {
        self.can_capture() && !self.batch_buffer.screenshots.is_empty()
    }

    fn should_take_screenshot(&self, now_ms: i64) -> bool {
        match self.status.last_screenshot_at_ms {
            Some(last) => now_ms - last >= self.config.screenshot_interval.as_millis() as i64,
            None => true,
        }
    }

    fn should_upload_batch(&self, now_ms: i64) -> bool {
        match self.status.last_batch_at_ms {
            Some(last) => now_ms - last >= self.config.batch_interval.as_millis() as i64,
            None => true,
        }
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

    fn retry_failed_requests(&mut self) -> CoreResult<()> {
        let requests = std::mem::take(&mut self.pending_requests);
        for request in requests {
            let _ = self.try_request(request)?;
        }
        Ok(())
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

    fn log_error(
        &self,
        message: &str,
        request: Option<&PendingRequest>,
        error: Option<&CoreError>,
    ) {
        let ts = self
            .platform
            .get_time_utc_ms()
            .map(|value| value.to_string())
            .unwrap_or_else(|_| "unknown-ts".to_string());
        let request_kind = request
            .map(|value| format!("{:?}", value.kind))
            .unwrap_or_else(|| "none".to_string());
        let request_id = request.map(|value| value.id.as_str()).unwrap_or("-");
        let error_text = error
            .map(ToString::to_string)
            .unwrap_or_else(|| "unknown error".to_string());
        let line = format!(
            "[{ts}] {message}; request_id={request_id}; kind={request_kind}; error={error_text}"
        );
        let _ = self.storage.append_error_log(&line);
        eprintln!("{line}");
    }
}

fn parse_content_hash(payload: &serde_json::Value) -> CoreResult<[u8; 32]> {
    let content_hash: [u8; 32] = serde_json::from_value(
        payload
            .get("content_hash")
            .cloned()
            .ok_or(CoreError::InvalidState("missing content_hash payload"))?,
    )?;
    Ok(content_hash)
}
