use crate::api::ApiClient;
use crate::audit::{derive_state, generate_local_id};
use crate::batch::BatchBuilder;
use crate::config::Config;
use crate::crypto::{
    CryptoEngine, prepare_log_batch_event, prepare_screenshot_batch_event, prepare_screenshot_event,
};
use crate::error::{CoreError, CoreResult};
use crate::image_pipeline::ImagePipeline;
use crate::lifecycle::{
    LifecycleObservation, LifecycleOrigin, LifecycleStatus, LifecycleTransition, ServicePingLog,
    ServiceRole, ServiceStopMarker, StopIntent, UserSessionState, apply_observation,
};
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
const SERVICE_PING_INTERVAL_MS: i64 = 60_000;
const SERVICE_PING_GRACE_MS: i64 = 10_000;
const STOP_ALERT_THRESHOLD_MS: i64 = 10_000;
const HIGH_RISK_LIFECYCLE_ALERT: f32 = 0.9;

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
            lifecycle: LifecycleStatus::for_platform(&config.platform_name),
        });
        status.is_running = true;
        status.is_authenticated = auth_state.device_credentials.is_some();
        status.device_id = auth_state
            .device_credentials
            .as_ref()
            .map(|device| device.device_id.clone());
        status.pending_request_count = audit_state.pending_request_count;
        status.lifecycle.capabilities =
            crate::lifecycle::LifecycleCapabilities::for_platform(&config.platform_name);

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

        if service.device_credentials.is_none() {
            service.clear_capture_schedule();
        }
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

        self.status.is_running = false;
        self.persist_state()
    }

    pub fn note_stop_requested_by_user(
        &mut self,
        role: ServiceRole,
        source: &str,
    ) -> CoreResult<()> {
        let requested_at_ms = self.platform.get_time_utc_ms()?;
        self.storage.save_stop_intent(&StopIntent {
            role,
            source: source.to_string(),
            requested_at_ms,
        })?;
        self.storage
            .append_lifecycle_observation(&LifecycleObservation::StopRequestedByUser {
                role,
                source: source.to_string(),
            })?;
        Ok(())
    }

    pub fn take_stop_intent(&mut self, role: ServiceRole) -> CoreResult<Option<StopIntent>> {
        let intent = self.storage.load_stop_intent()?;
        if intent.as_ref().is_some_and(|intent| intent.role == role) {
            self.storage.clear_stop_intent()?;
            Ok(intent)
        } else {
            Ok(None)
        }
    }

    pub fn record_lifecycle_observation(
        &mut self,
        observation: LifecycleObservation,
    ) -> CoreResult<Vec<LifecycleTransition>> {
        self.storage.append_lifecycle_observation(&observation)?;
        let observed_at_ms = self.platform.get_time_utc_ms()?;
        let result = apply_observation(&self.status.lifecycle, &observation);
        self.status.lifecycle = result.status;
        let base_ts = match &observation {
            LifecycleObservation::BootObserved {
                booted_at_ms: Some(booted_at_ms),
                ..
            } => *booted_at_ms,
            _ => observed_at_ms,
        };

        self.handle_lifecycle_observation_effects(
            &observation,
            &result.transitions,
            observed_at_ms,
        )?;

        for (index, transition) in result.transitions.iter().enumerate() {
            self.enqueue_lifecycle_transition_log(
                transition,
                base_ts.saturating_add(index as i64),
            )?;
        }

        self.persist_state()?;
        Ok(result.transitions)
    }

    pub fn next_service_ping_due_at_ms(&self, role: ServiceRole) -> CoreResult<Option<i64>> {
        if self.device_credentials.is_none() {
            return Ok(None);
        }

        let due_at_ms = self
            .storage
            .load_last_service_ping(role)?
            .map(|ping| ping.pinged_at_ms.saturating_add(SERVICE_PING_INTERVAL_MS))
            .unwrap_or(self.platform.get_time_utc_ms()?);
        Ok(Some(due_at_ms))
    }

    pub fn record_service_ping_if_due(
        &mut self,
        role: ServiceRole,
        detected_by: &str,
    ) -> CoreResult<bool> {
        if self.device_credentials.is_none() {
            return Ok(false);
        }

        let now_ms = self.platform.get_time_utc_ms()?;
        let previous_ping = self.storage.load_last_service_ping(role)?;
        if previous_ping
            .as_ref()
            .is_some_and(|ping| now_ms < ping.pinged_at_ms.saturating_add(SERVICE_PING_INTERVAL_MS))
        {
            return Ok(false);
        }

        let gap_ms = previous_ping
            .as_ref()
            .map(|ping| now_ms.saturating_sub(ping.pinged_at_ms));
        let risk =
            if gap_ms.is_some_and(|gap| gap > SERVICE_PING_INTERVAL_MS + SERVICE_PING_GRACE_MS) {
                HIGH_RISK_LIFECYCLE_ALERT
            } else {
                0.0
            };
        let ping_log = ServicePingLog {
            role,
            pinged_at_ms: now_ms,
            gap_ms,
            risk,
            detected_by: detected_by.to_string(),
        };
        self.storage.append_service_ping_log(&ping_log)?;
        self.storage.save_last_service_ping(&ping_log)?;
        Ok(true)
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
        self.storage.clear_all_service_stop_markers()?;
        self.storage.clear_all_service_pings()?;
        self.storage.clear_stop_intent()?;

        self.user_access_token = Some(access_token.clone());
        self.device_credentials = Some(device.clone());
        self.post_login_proof_batches_remaining = POST_LOGIN_PROOF_BATCH_COUNT;
        self.clear_capture_schedule();
        self.status.is_authenticated = true;
        self.status.device_id = Some(device.device_id.clone());
        self.persist_auth_state()?;

        self.refresh_device_settings()?;
        self.record_lifecycle_observation(LifecycleObservation::UserSessionChanged {
            state: UserSessionState::LoggedIn,
            origin: LifecycleOrigin::UserRequested,
            detected_by: "core_login".to_string(),
        })?;

        Ok(LoginStatus {
            access_token,
            device: Some(device),
        })
    }

    pub fn logout(&mut self) -> CoreResult<()> {
        self.ensure_running()?;

        if self.device_credentials.is_some() {
            self.storage.clear_audit_records()?;
            self.record_lifecycle_observation(LifecycleObservation::UserSessionChanged {
                state: UserSessionState::LoggedOut,
                origin: LifecycleOrigin::UserRequested,
                detected_by: "core_logout".to_string(),
            })?;
        }

        if let Some(token) = self.user_access_token.as_deref() {
            let _ = self.api.logout(token);
        }

        self.user_access_token = None;
        self.device_credentials = None;
        self.post_login_proof_batches_remaining = 0;
        self.device_settings = None;
        self.storage.clear_all_service_stop_markers()?;
        self.storage.clear_all_service_pings()?;
        self.storage.clear_stop_intent()?;
        self.status.is_authenticated = false;
        self.status.device_id = None;
        self.clear_capture_schedule();
        self.persist_state()
    }

    pub fn status(&self) -> CoreResult<ServiceStatus> {
        let mut status = self
            .storage
            .load_status()?
            .unwrap_or_else(|| self.status.clone());
        status.pending_request_count = self.load_audit_state()?.pending_request_count;
        status.lifecycle.capabilities =
            crate::lifecycle::LifecycleCapabilities::for_platform(&self.config.platform_name);
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

    fn append_direct_log(&mut self, log: LogEntry) -> CoreResult<AuditLogItem> {
        let item = self.append_audit_log(false, false, AuditLogPayload::for_direct_log(log))?;
        let _ = self.try_upload_direct_log(&item);
        Ok(item)
    }

    fn append_batch_log(
        &mut self,
        ts: i64,
        kind: &str,
        risk: Option<f32>,
        data: EventData,
    ) -> CoreResult<AuditLogItem> {
        let event = prepare_log_batch_event(ts, kind, risk, data)?;
        self.enqueue_batch_event(event, false)
    }

    fn lifecycle_transition_log(
        &mut self,
        transition: &LifecycleTransition,
        ts: i64,
    ) -> CoreResult<LogEntry> {
        let mut data = EventData::default();
        data.insert(
            "domain",
            serde_json::Value::String(transition.domain.as_str().to_string()),
        );
        data.insert("from", serde_json::Value::String(transition.from.clone()));
        data.insert("to", serde_json::Value::String(transition.to.clone()));
        data.insert(
            "origin",
            serde_json::Value::String(transition.origin.as_str().to_string()),
        );
        data.insert(
            "detected_by",
            serde_json::Value::String(transition.detected_by.clone()),
        );
        data.insert(
            "confidence",
            serde_json::Value::String(transition.confidence.as_str().to_string()),
        );
        if let Some(role) = transition.service_role {
            data.insert(
                "service_role",
                serde_json::Value::String(role.as_str().to_string()),
            );
        }
        Ok(LogEntry {
            ts,
            kind: "lifecycle_transition".to_string(),
            risk: Some(transition.risk),
            data,
        })
    }

    fn enqueue_lifecycle_transition_log(
        &mut self,
        transition: &LifecycleTransition,
        ts: i64,
    ) -> CoreResult<()> {
        let log = self.lifecycle_transition_log(transition, ts)?;
        let _ = self.append_batch_log(log.ts, &log.kind, log.risk, log.data)?;
        Ok(())
    }

    fn lifecycle_alert_log(&self, ts: i64, risk: f32, data: EventData) -> LogEntry {
        LogEntry {
            ts,
            kind: "lifecycle_alert".to_string(),
            risk: Some(risk),
            data,
        }
    }

    fn emit_lifecycle_alert(&mut self, log: LogEntry) -> CoreResult<()> {
        if log.risk.unwrap_or_default() >= HIGH_RISK_LIFECYCLE_ALERT {
            let _ = self.append_direct_log(log)?;
        } else {
            let _ = self.append_batch_log(log.ts, &log.kind, log.risk, log.data)?;
        }
        Ok(())
    }

    fn handle_lifecycle_observation_effects(
        &mut self,
        observation: &LifecycleObservation,
        transitions: &[LifecycleTransition],
        observed_at_ms: i64,
    ) -> CoreResult<()> {
        match observation {
            LifecycleObservation::ServiceStopObserved { role, .. } => {
                let Some(service_transition) = transitions.iter().find(|transition| {
                    transition.service_role == Some(*role) && transition.to == "stopped"
                }) else {
                    return Ok(());
                };

                self.storage.save_service_stop_marker(&ServiceStopMarker {
                    role: *role,
                    origin: service_transition.origin,
                    stopped_at_ms: observed_at_ms,
                })?;

                if self.device_credentials.is_some()
                    && service_transition.origin == LifecycleOrigin::UserRequested
                {
                    let mut data = EventData::default();
                    data.insert(
                        "alert_reason",
                        serde_json::Value::String("user_initiated_stop".to_string()),
                    );
                    data.insert(
                        "service_role",
                        serde_json::Value::String(role.as_str().to_string()),
                    );
                    data.insert(
                        "origin",
                        serde_json::Value::String(service_transition.origin.as_str().to_string()),
                    );
                    data.insert(
                        "detected_by",
                        serde_json::Value::String(service_transition.detected_by.clone()),
                    );
                    self.emit_lifecycle_alert(self.lifecycle_alert_log(
                        observed_at_ms,
                        HIGH_RISK_LIFECYCLE_ALERT,
                        data,
                    ))?;
                }
            }
            LifecycleObservation::ServiceStarted { role, detected_by } => {
                let Some(_service_transition) = transitions.iter().find(|transition| {
                    transition.service_role == Some(*role) && transition.to == "running"
                }) else {
                    return Ok(());
                };

                let marker = self.storage.load_service_stop_marker(*role)?;
                self.storage.clear_service_stop_marker(*role)?;

                if self.device_credentials.is_none() {
                    return Ok(());
                }

                match marker {
                    None => match self.storage.load_last_service_ping(*role)? {
                        Some(last_ping)
                            if observed_at_ms.saturating_sub(last_ping.pinged_at_ms)
                                > SERVICE_PING_INTERVAL_MS + SERVICE_PING_GRACE_MS =>
                        {
                            let ping_gap_ms = observed_at_ms.saturating_sub(last_ping.pinged_at_ms);
                            let mut data = EventData::default();
                            data.insert(
                                "alert_reason",
                                serde_json::Value::String(
                                    "missing_stop_marker_after_ping_gap".to_string(),
                                ),
                            );
                            data.insert(
                                "service_role",
                                serde_json::Value::String(role.as_str().to_string()),
                            );
                            data.insert(
                                "detected_by",
                                serde_json::Value::String(detected_by.clone()),
                            );
                            data.insert("ping_gap_ms", serde_json::Value::from(ping_gap_ms));
                            data.insert(
                                "ping_threshold_ms",
                                serde_json::Value::from(
                                    SERVICE_PING_INTERVAL_MS + SERVICE_PING_GRACE_MS,
                                ),
                            );
                            self.emit_lifecycle_alert(self.lifecycle_alert_log(
                                observed_at_ms,
                                HIGH_RISK_LIFECYCLE_ALERT,
                                data,
                            ))?;
                        }
                        Some(_) => {}
                        None => {
                            let mut data = EventData::default();
                            data.insert(
                                "alert_reason",
                                serde_json::Value::String("missing_stop_marker".to_string()),
                            );
                            data.insert(
                                "service_role",
                                serde_json::Value::String(role.as_str().to_string()),
                            );
                            data.insert(
                                "detected_by",
                                serde_json::Value::String(detected_by.clone()),
                            );
                            self.emit_lifecycle_alert(self.lifecycle_alert_log(
                                observed_at_ms,
                                HIGH_RISK_LIFECYCLE_ALERT,
                                data,
                            ))?;
                        }
                    },
                    Some(marker)
                        if marker.origin == LifecycleOrigin::UserRequested
                            || marker.origin == LifecycleOrigin::SystemShutdown => {}
                    Some(marker) => {
                        let downtime_ms = observed_at_ms.saturating_sub(marker.stopped_at_ms);
                        if downtime_ms > STOP_ALERT_THRESHOLD_MS {
                            let mut data = EventData::default();
                            data.insert(
                                "alert_reason",
                                serde_json::Value::String("extended_service_stop".to_string()),
                            );
                            data.insert(
                                "service_role",
                                serde_json::Value::String(role.as_str().to_string()),
                            );
                            data.insert(
                                "origin",
                                serde_json::Value::String(marker.origin.as_str().to_string()),
                            );
                            data.insert("downtime_ms", serde_json::Value::from(downtime_ms));
                            data.insert(
                                "threshold_ms",
                                serde_json::Value::from(STOP_ALERT_THRESHOLD_MS),
                            );
                            data.insert(
                                "detected_by",
                                serde_json::Value::String(detected_by.clone()),
                            );
                            self.emit_lifecycle_alert(self.lifecycle_alert_log(
                                observed_at_ms,
                                HIGH_RISK_LIFECYCLE_ALERT,
                                data,
                            ))?;
                        }
                    }
                }
            }
            LifecycleObservation::UserSessionChanged {
                state,
                origin,
                detected_by,
            } => {
                let Some(session_transition) = transitions.iter().find(|transition| {
                    transition.domain.as_str() == "user_session" && transition.to == state.as_str()
                }) else {
                    return Ok(());
                };

                if self.device_credentials.is_some() && *state == UserSessionState::LoggedOut {
                    let mut data = EventData::default();
                    data.insert(
                        "alert_reason",
                        serde_json::Value::String("user_session_logout".to_string()),
                    );
                    data.insert(
                        "origin",
                        serde_json::Value::String(origin.as_str().to_string()),
                    );
                    data.insert(
                        "detected_by",
                        serde_json::Value::String(detected_by.clone()),
                    );
                    data.insert(
                        "from",
                        serde_json::Value::String(session_transition.from.clone()),
                    );
                    data.insert(
                        "to",
                        serde_json::Value::String(session_transition.to.clone()),
                    );
                    self.emit_lifecycle_alert(self.lifecycle_alert_log(
                        observed_at_ms,
                        HIGH_RISK_LIFECYCLE_ALERT,
                        data,
                    ))?;
                }
            }
            _ => {}
        }

        Ok(())
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
        self.status.lifecycle.capabilities =
            crate::lifecycle::LifecycleCapabilities::for_platform(&self.config.platform_name);

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
        self.storage.clear_all_service_stop_markers()?;
        self.storage.clear_all_service_pings()?;
        self.storage.clear_stop_intent()?;
        self.status.is_authenticated = false;
        self.status.device_id = None;
        self.clear_capture_schedule();
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
        if self.device_credentials.is_none() || proof_burst_started {
            self.clear_capture_schedule();
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
        if self.device_credentials.is_none() {
            return now_ms + self.config.screenshot_interval.as_millis() as i64;
        }

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

    fn clear_capture_schedule(&mut self) {
        self.status.last_screenshot_at_ms = None;
        self.status.last_batch_at_ms = None;
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
    use std::time::Duration;

    use super::*;

    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[derive(Clone)]
    struct TestPlatform {
        now_ms: Arc<AtomicI64>,
    }

    impl PlatformHooks for TestPlatform {
        fn take_screenshot(&self) -> CoreResult<Screenshot> {
            Ok(Screenshot {
                captured_at_ms: 0,
                bytes: Vec::new(),
                content_type: "image/png".to_string(),
            })
        }

        fn get_time_utc_ms(&self) -> CoreResult<i64> {
            Ok(self.now_ms.load(Ordering::Relaxed))
        }
    }

    impl TestPlatform {
        fn new(now_ms: i64) -> Self {
            Self {
                now_ms: Arc::new(AtomicI64::new(now_ms)),
            }
        }

        fn set_time_ms(&self, now_ms: i64) {
            self.now_ms.store(now_ms, Ordering::Relaxed);
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
        let platform = TestPlatform::new(0);
        MonitorService {
            api: ApiClient::new(&config).expect("create api client"),
            config,
            platform,
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
                lifecycle: LifecycleStatus::for_platform("test"),
            },
        }
    }

    fn authenticate_service(service: &mut MonitorService<TestPlatform>) {
        service.device_credentials = Some(DeviceCredentials {
            device_id: "device-1".to_string(),
            access_token: "device-access".to_string(),
            refresh_token: "device-refresh".to_string(),
        });
        service.status.is_authenticated = true;
        service.status.device_id = Some("device-1".to_string());
    }

    fn lifecycle_direct_logs(service: &MonitorService<TestPlatform>) -> Vec<LogEntry> {
        service
            .storage
            .load_audit_records_at(0)
            .expect("load audit records")
            .into_iter()
            .filter_map(|record| match record.record {
                AuditRecord::Log { log, .. } => log.as_direct_log().cloned(),
                AuditRecord::LocalLog { .. } => None,
                AuditRecord::HashUploaded { .. }
                | AuditRecord::LogUploaded { .. }
                | AuditRecord::BatchUploaded { .. } => None,
            })
            .collect()
    }

    fn lifecycle_batch_logs(service: &MonitorService<TestPlatform>) -> Vec<LogEntry> {
        service
            .storage
            .load_audit_records_at(0)
            .expect("load audit records")
            .into_iter()
            .filter_map(|record| match record.record {
                AuditRecord::Log { log, .. } => {
                    log.as_batch_event().map(|event| event.event.clone())
                }
                AuditRecord::LocalLog { .. } => None,
                AuditRecord::HashUploaded { .. }
                | AuditRecord::LogUploaded { .. }
                | AuditRecord::BatchUploaded { .. } => None,
            })
            .collect()
    }

    fn lifecycle_audit_logs(service: &MonitorService<TestPlatform>) -> Vec<LogEntry> {
        let mut logs = lifecycle_direct_logs(service);
        logs.extend(lifecycle_batch_logs(service));
        logs.sort_by_key(|log| log.ts);
        logs
    }

    fn service_ping_state(
        service: &MonitorService<TestPlatform>,
        role: ServiceRole,
    ) -> Option<ServicePingLog> {
        service
            .storage
            .load_last_service_ping(role)
            .expect("load last service ping")
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
    fn setup_clears_stale_capture_schedule_when_logged_out() {
        let state_dir = temp_state_dir();
        let storage = FileStateStore::new(&state_dir).expect("create file state store");
        storage
            .save_status(&ServiceStatus {
                is_authenticated: false,
                is_running: true,
                device_id: None,
                last_loop_at_ms: Some(1),
                last_screenshot_at_ms: Some(1000),
                last_batch_at_ms: Some(2000),
                pending_request_count: 0,
                lifecycle: LifecycleStatus::for_platform("test"),
            })
            .expect("save stale status");

        let service = MonitorService::setup(test_config(state_dir.clone()), TestPlatform::new(0))
            .expect("setup service");

        assert_eq!(service.status.last_screenshot_at_ms, None);
        assert_eq!(service.status.last_batch_at_ms, None);

        let persisted_status = storage
            .load_status()
            .expect("load persisted status")
            .expect("persisted status");
        assert_eq!(persisted_status.last_screenshot_at_ms, None);
        assert_eq!(persisted_status.last_batch_at_ms, None);

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn next_run_stays_in_future_when_logged_out_with_stale_timestamps() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());
        service.status.last_screenshot_at_ms = Some(1);
        service.status.last_batch_at_ms = Some(1);

        let next_run_at_ms = service.next_run_at_ms(10_000);

        assert_eq!(next_run_at_ms, 10_000 + 300_000);

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
                lifecycle: LifecycleStatus::for_platform("test"),
            })
            .expect("save stale status");
        service
            .storage
            .append_audit_log_record(&AuditRecord::Log {
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
            .append_audit_log_record(&AuditRecord::Log {
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

    #[test]
    fn lifecycle_observations_emit_shutdown_transition_log() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());

        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStarted {
                role: ServiceRole::PrimaryService,
                detected_by: "linux_service".to_string(),
            })
            .expect("record service start");
        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStopObserved {
                role: ServiceRole::PrimaryService,
                raw_reason: "SIGTERM".to_string(),
                shutdown_in_progress: true,
                explicit_user_stop: false,
                detected_by: "signal_plus_system_state".to_string(),
            })
            .expect("record service stop");

        let logs = lifecycle_batch_logs(&service);
        assert_eq!(logs.len(), 3);

        let stop_log = logs.get(1).expect("service stop log");
        assert_eq!(stop_log.kind, "lifecycle_transition");
        assert_eq!(stop_log.risk, Some(0.0));
        assert_eq!(
            stop_log.data.get("domain"),
            Some(&serde_json::Value::String("primary_service".to_string()))
        );
        assert_eq!(
            stop_log.data.get("from"),
            Some(&serde_json::Value::String("running".to_string()))
        );
        assert_eq!(
            stop_log.data.get("to"),
            Some(&serde_json::Value::String("stopped".to_string()))
        );
        assert_eq!(
            stop_log.data.get("origin"),
            Some(&serde_json::Value::String("system_shutdown".to_string()))
        );

        let power_log = logs.get(2).expect("power transition log");
        assert_eq!(power_log.kind, "lifecycle_transition");
        assert_eq!(power_log.risk, Some(0.0));
        assert_eq!(
            power_log.data.get("domain"),
            Some(&serde_json::Value::String("computer_power".to_string()))
        );
        assert_eq!(
            power_log.data.get("from"),
            Some(&serde_json::Value::String("running".to_string()))
        );
        assert_eq!(
            power_log.data.get("to"),
            Some(&serde_json::Value::String("shutting_down".to_string()))
        );
        assert_eq!(power_log.ts, stop_log.ts.saturating_add(1));

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn lifecycle_observations_emit_unknown_stop_transition_log() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());

        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStarted {
                role: ServiceRole::PrimaryService,
                detected_by: "linux_service".to_string(),
            })
            .expect("record service start");
        service
            .record_lifecycle_observation(LifecycleObservation::ProcessMissing {
                role: ServiceRole::PrimaryService,
                had_expected_runtime: true,
                detected_by: "missing_process".to_string(),
            })
            .expect("record missing process");

        let logs = lifecycle_batch_logs(&service);
        assert_eq!(logs.len(), 2);

        let crash_log = logs.last().expect("crash log");
        assert_eq!(crash_log.kind, "lifecycle_transition");
        assert_eq!(crash_log.risk, Some(0.5));
        assert_eq!(
            crash_log.data.get("to"),
            Some(&serde_json::Value::String("crashed".to_string()))
        );
        assert_eq!(
            crash_log.data.get("origin"),
            Some(&serde_json::Value::String("crash_or_kill".to_string()))
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn lifecycle_observations_emit_explicit_user_stop_alert_and_batched_transition() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());

        service
            .note_stop_requested_by_user(ServiceRole::PrimaryService, "tray_close")
            .expect("record stop intent");
        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStarted {
                role: ServiceRole::PrimaryService,
                detected_by: "macos_launch_agent".to_string(),
            })
            .expect("record service start");
        authenticate_service(&mut service);
        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStopObserved {
                role: ServiceRole::PrimaryService,
                raw_reason: "launchctl_bootout".to_string(),
                shutdown_in_progress: false,
                explicit_user_stop: true,
                detected_by: "stop_intent".to_string(),
            })
            .expect("record explicit stop");

        let direct_logs = lifecycle_direct_logs(&service);
        assert_eq!(direct_logs.len(), 1);

        let alert_log = direct_logs.last().expect("explicit stop alert");
        assert_eq!(alert_log.kind, "lifecycle_alert");
        assert_eq!(alert_log.risk, Some(0.9));
        assert_eq!(
            alert_log.data.get("alert_reason"),
            Some(&serde_json::Value::String(
                "user_initiated_stop".to_string()
            ))
        );
        assert_eq!(
            alert_log.data.get("service_role"),
            Some(&serde_json::Value::String("primary_service".to_string()))
        );

        let batch_logs = lifecycle_batch_logs(&service);
        let stop_log = batch_logs.last().expect("explicit stop transition");
        assert_eq!(stop_log.kind, "lifecycle_transition");
        assert_eq!(stop_log.risk, Some(0.0));
        assert_eq!(
            stop_log.data.get("origin"),
            Some(&serde_json::Value::String("user_requested".to_string()))
        );
        assert_eq!(
            stop_log.data.get("from"),
            Some(&serde_json::Value::String("running".to_string()))
        );
        assert_eq!(
            stop_log.data.get("to"),
            Some(&serde_json::Value::String("stopped".to_string()))
        );

        let audit_logs = lifecycle_audit_logs(&service);
        assert_eq!(
            audit_logs
                .iter()
                .filter(|log| {
                    log.kind == "lifecycle_transition"
                        && log.data.get("origin")
                            == Some(&serde_json::Value::String("user_requested".to_string()))
                        && log.data.get("to")
                            == Some(&serde_json::Value::String("stopped".to_string()))
                })
                .count(),
            1
        );

        let stop_intent = service
            .storage
            .load_stop_intent()
            .expect("load stop intent")
            .expect("persisted stop intent");
        assert_eq!(stop_intent.source, "tray_close");

        let stop_marker = service
            .storage
            .load_service_stop_marker(ServiceRole::PrimaryService)
            .expect("load stop marker")
            .expect("persisted stop marker");
        assert_eq!(stop_marker.origin, LifecycleOrigin::UserRequested);

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn lifecycle_observations_emit_suspend_and_resume_transition_logs() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());

        service
            .record_lifecycle_observation(LifecycleObservation::ComputerPowerChanged {
                state: crate::lifecycle::ComputerPowerState::Suspended,
                origin: crate::lifecycle::LifecycleOrigin::SystemSuspend,
                detected_by: "login1_prepare_for_sleep".to_string(),
                confidence: crate::lifecycle::LifecycleConfidence::Confirmed,
            })
            .expect("record suspend");
        service
            .record_lifecycle_observation(LifecycleObservation::ComputerPowerChanged {
                state: crate::lifecycle::ComputerPowerState::Running,
                origin: crate::lifecycle::LifecycleOrigin::SystemSuspend,
                detected_by: "login1_prepare_for_sleep".to_string(),
                confidence: crate::lifecycle::LifecycleConfidence::Confirmed,
            })
            .expect("record resume");

        let logs = lifecycle_batch_logs(&service);
        assert_eq!(logs.len(), 2);

        let suspend_log = &logs[0];
        assert_eq!(suspend_log.kind, "lifecycle_transition");
        assert_eq!(suspend_log.risk, Some(0.0));
        assert_eq!(
            suspend_log.data.get("domain"),
            Some(&serde_json::Value::String("computer_power".to_string()))
        );
        assert_eq!(
            suspend_log.data.get("to"),
            Some(&serde_json::Value::String("suspended".to_string()))
        );
        assert_eq!(
            suspend_log.data.get("origin"),
            Some(&serde_json::Value::String("system_suspend".to_string()))
        );

        let resume_log = &logs[1];
        assert_eq!(
            resume_log.data.get("from"),
            Some(&serde_json::Value::String("suspended".to_string()))
        );
        assert_eq!(
            resume_log.data.get("to"),
            Some(&serde_json::Value::String("running".to_string()))
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn user_session_login_observation_emits_batched_lifecycle_transition() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());

        service
            .record_lifecycle_observation(LifecycleObservation::UserSessionChanged {
                state: UserSessionState::LoggedIn,
                origin: LifecycleOrigin::UserRequested,
                detected_by: "core_login".to_string(),
            })
            .expect("record login session change");

        let direct_logs = lifecycle_direct_logs(&service);
        assert!(direct_logs.is_empty());

        let batch_logs = lifecycle_batch_logs(&service);
        assert_eq!(batch_logs.len(), 1);
        let login_log = &batch_logs[0];
        assert_eq!(login_log.kind, "lifecycle_transition");
        assert_eq!(login_log.risk, Some(0.0));
        assert_eq!(
            login_log.data.get("domain"),
            Some(&serde_json::Value::String("user_session".to_string()))
        );
        assert_eq!(
            login_log.data.get("from"),
            Some(&serde_json::Value::String("unknown".to_string()))
        );
        assert_eq!(
            login_log.data.get("to"),
            Some(&serde_json::Value::String("logged_in".to_string()))
        );
        assert_eq!(
            login_log.data.get("origin"),
            Some(&serde_json::Value::String("user_requested".to_string()))
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn logout_emits_high_risk_lifecycle_alert_and_transition() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());
        authenticate_service(&mut service);
        service.user_access_token = Some("user-token".to_string());
        service.status.lifecycle.snapshot.user_session = UserSessionState::LoggedIn;
        service
            .storage
            .append_audit_log_record(&AuditRecord::Log {
                local_id: "stale-log".to_string(),
                should_be_in_batch: false,
                requires_hash_upload: false,
                log: AuditLogPayload::for_direct_log(LogEntry {
                    ts: 1,
                    kind: "system_event".to_string(),
                    risk: None,
                    data: EventData::from_pairs([("event".to_string(), "stale".to_string())]),
                }),
            })
            .expect("append stale log");

        service.logout().expect("logout succeeds");

        let direct_logs = lifecycle_direct_logs(&service);
        assert_eq!(direct_logs.len(), 1);
        let alert_log = &direct_logs[0];
        assert_eq!(alert_log.kind, "lifecycle_alert");
        assert_eq!(alert_log.risk, Some(0.9));
        assert_eq!(
            alert_log.data.get("alert_reason"),
            Some(&serde_json::Value::String(
                "user_session_logout".to_string()
            ))
        );
        assert_eq!(
            alert_log.data.get("to"),
            Some(&serde_json::Value::String("logged_out".to_string()))
        );

        let batch_logs = lifecycle_batch_logs(&service);
        assert_eq!(batch_logs.len(), 1);
        let logout_log = &batch_logs[0];
        assert_eq!(logout_log.kind, "lifecycle_transition");
        assert_eq!(logout_log.risk, Some(0.0));
        assert_eq!(
            logout_log.data.get("domain"),
            Some(&serde_json::Value::String("user_session".to_string()))
        );
        assert_eq!(
            logout_log.data.get("from"),
            Some(&serde_json::Value::String("logged_in".to_string()))
        );
        assert_eq!(
            logout_log.data.get("to"),
            Some(&serde_json::Value::String("logged_out".to_string()))
        );
        assert_eq!(
            logout_log.data.get("origin"),
            Some(&serde_json::Value::String("user_requested".to_string()))
        );

        let audit_logs = lifecycle_audit_logs(&service);
        assert!(audit_logs.iter().all(|log| !(log.kind == "system_event"
            && log.data.get("event") == Some(&serde_json::Value::String("stale".to_string())))));
        assert!(!service.status.is_authenticated);

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn boot_observed_uses_supplied_boot_timestamp_for_started_log() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());
        let booted_at_ms = 1_776_519_136_000_i64;

        service
            .record_lifecycle_observation(LifecycleObservation::BootObserved {
                boot_marker: "boot-123".to_string(),
                booted_at_ms: Some(booted_at_ms),
                detected_by: "boot_id_change".to_string(),
            })
            .expect("record boot");

        let logs = lifecycle_batch_logs(&service);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].ts, booted_at_ms);
        assert_eq!(
            logs[0].data.get("domain"),
            Some(&serde_json::Value::String("computer_power".to_string()))
        );
        assert_eq!(
            logs[0].data.get("to"),
            Some(&serde_json::Value::String("started".to_string()))
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn service_ping_records_local_gap_risk_without_uploading() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());
        authenticate_service(&mut service);

        service.platform.set_time_ms(1_000);
        assert!(
            service
                .record_service_ping_if_due(ServiceRole::PrimaryService, "test_ping")
                .expect("record initial ping")
        );
        let first_ping =
            service_ping_state(&service, ServiceRole::PrimaryService).expect("first ping state");
        assert_eq!(first_ping.pinged_at_ms, 1_000);
        assert_eq!(first_ping.gap_ms, None);
        assert_eq!(first_ping.risk, 0.0);

        service.platform.set_time_ms(75_500);
        assert!(
            service
                .record_service_ping_if_due(ServiceRole::PrimaryService, "test_ping")
                .expect("record delayed ping")
        );
        let delayed_ping =
            service_ping_state(&service, ServiceRole::PrimaryService).expect("delayed ping state");
        assert_eq!(delayed_ping.gap_ms, Some(74_500));
        assert_eq!(delayed_ping.risk, 0.9);
        assert!(
            lifecycle_direct_logs(&service)
                .iter()
                .all(|log| log.kind != "lifecycle_alert")
        );
        assert!(
            lifecycle_batch_logs(&service)
                .iter()
                .all(|log| log.kind != "lifecycle_alert")
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn lifecycle_start_after_long_unexpected_stop_emits_direct_high_risk_alert() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());

        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStarted {
                role: ServiceRole::PrimaryService,
                detected_by: "linux_service".to_string(),
            })
            .expect("record initial service start");
        authenticate_service(&mut service);
        service.platform.set_time_ms(1_000);
        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStopObserved {
                role: ServiceRole::PrimaryService,
                raw_reason: "SIGTERM".to_string(),
                shutdown_in_progress: false,
                explicit_user_stop: false,
                detected_by: "signal_plus_system_state".to_string(),
            })
            .expect("record stop");
        service.platform.set_time_ms(12_500);
        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStarted {
                role: ServiceRole::PrimaryService,
                detected_by: "linux_service".to_string(),
            })
            .expect("record restart");

        let direct_logs = lifecycle_direct_logs(&service);
        let alert_log = direct_logs
            .iter()
            .rev()
            .find(|log| log.kind == "lifecycle_alert")
            .expect("direct lifecycle alert");
        assert_eq!(alert_log.risk, Some(0.9));
        assert_eq!(
            alert_log.data.get("alert_reason"),
            Some(&serde_json::Value::String(
                "extended_service_stop".to_string()
            ))
        );
        assert_eq!(
            alert_log.data.get("downtime_ms"),
            Some(&serde_json::Value::from(11_500_i64))
        );
        assert!(
            service
                .storage
                .load_service_stop_marker(ServiceRole::PrimaryService)
                .expect("load stop marker")
                .is_none()
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn lifecycle_start_without_marker_and_recent_ping_does_not_alert() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());
        authenticate_service(&mut service);

        service.platform.set_time_ms(1_000);
        service
            .record_service_ping_if_due(ServiceRole::PrimaryService, "test_ping")
            .expect("record initial ping");
        service.platform.set_time_ms(65_000);
        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStarted {
                role: ServiceRole::PrimaryService,
                detected_by: "linux_service".to_string(),
            })
            .expect("record start without marker");

        assert!(
            lifecycle_direct_logs(&service)
                .iter()
                .all(|log| log.kind != "lifecycle_alert")
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn lifecycle_start_without_marker_and_stale_ping_emits_direct_high_risk_alert() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());
        authenticate_service(&mut service);

        service.platform.set_time_ms(1_000);
        service
            .record_service_ping_if_due(ServiceRole::PrimaryService, "test_ping")
            .expect("record initial ping");
        service.platform.set_time_ms(75_000);
        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStarted {
                role: ServiceRole::PrimaryService,
                detected_by: "linux_service".to_string(),
            })
            .expect("record start without marker");

        let direct_logs = lifecycle_direct_logs(&service);
        let alert_log = direct_logs
            .iter()
            .find(|log| log.kind == "lifecycle_alert")
            .expect("direct lifecycle alert");
        assert_eq!(alert_log.risk, Some(0.9));
        assert_eq!(
            alert_log.data.get("alert_reason"),
            Some(&serde_json::Value::String(
                "missing_stop_marker_after_ping_gap".to_string()
            ))
        );
        assert_eq!(
            alert_log.data.get("ping_gap_ms"),
            Some(&serde_json::Value::from(74_000_i64))
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn lifecycle_start_after_shutdown_stop_does_not_alert() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());

        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStarted {
                role: ServiceRole::PrimaryService,
                detected_by: "linux_service".to_string(),
            })
            .expect("record initial start");
        authenticate_service(&mut service);
        service.platform.set_time_ms(2_000);
        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStopObserved {
                role: ServiceRole::PrimaryService,
                raw_reason: "SIGTERM".to_string(),
                shutdown_in_progress: true,
                explicit_user_stop: false,
                detected_by: "signal_plus_system_state".to_string(),
            })
            .expect("record shutdown stop");
        service.platform.set_time_ms(25_000);
        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStarted {
                role: ServiceRole::PrimaryService,
                detected_by: "linux_service".to_string(),
            })
            .expect("record restart");

        assert!(
            lifecycle_direct_logs(&service)
                .iter()
                .all(|log| log.kind != "lifecycle_alert")
        );
        assert!(
            lifecycle_batch_logs(&service)
                .iter()
                .all(|log| log.kind != "lifecycle_alert")
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn lifecycle_start_after_user_stop_does_not_alert_again() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());

        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStarted {
                role: ServiceRole::PrimaryService,
                detected_by: "linux_service".to_string(),
            })
            .expect("record initial service start");
        authenticate_service(&mut service);
        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStopObserved {
                role: ServiceRole::PrimaryService,
                raw_reason: "SIGTERM".to_string(),
                shutdown_in_progress: false,
                explicit_user_stop: true,
                detected_by: "signal_plus_system_state".to_string(),
            })
            .expect("record explicit user stop");

        let direct_logs = lifecycle_direct_logs(&service);
        assert_eq!(
            direct_logs
                .iter()
                .filter(|log| log.kind == "lifecycle_alert")
                .count(),
            1
        );

        service.platform.set_time_ms(15_000);
        service
            .record_lifecycle_observation(LifecycleObservation::ServiceStarted {
                role: ServiceRole::PrimaryService,
                detected_by: "linux_service".to_string(),
            })
            .expect("record restart");

        let direct_logs = lifecycle_direct_logs(&service);
        assert_eq!(
            direct_logs
                .iter()
                .filter(|log| log.kind == "lifecycle_alert")
                .count(),
            1
        );

        let _ = fs::remove_dir_all(state_dir);
    }
}
