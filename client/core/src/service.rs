use crate::api::{ApiTransport, ReqwestApiClient};
use crate::auth::Auth;
use crate::config::Config;
use crate::crypto::{prepare_log_batch_event, prepare_screenshot_batch_event};
use crate::error::{CoreError, CoreResult};
use crate::events::screenshot::image_pipeline::ImagePipeline;
use crate::events::{
    Event, EventLoop, HIGH_RISK_LIFECYCLE_ALERT, LifecycleObserver, Observers,
    SERVICE_PING_GRACE_MS, SERVICE_PING_INTERVAL_MS, ScreenshotConfig, ScreenshotObserver,
    UploadConfig, UploadObserver, log_error,
};
use crate::lifecycle::{
    LifecycleObservation, LifecycleTransition, ServicePingLog, ServiceRole, StopIntent,
    UserSessionState,
};
use crate::model::{EventData, LogEntry, LoginStatus, LoopOutcome, ServiceStatus};
use crate::platform::PlatformHooks;
use crate::storage::FileStateStore;

pub struct MonitorService<P: PlatformHooks + Clone, A: ApiTransport + Clone = ReqwestApiClient> {
    config: Config,
    platform: P,
    api: A,
    storage: FileStateStore,
    status: ServiceStatus,
    pub(crate) auth: Auth,
    pub(crate) event_loop: EventLoop<P, A>,
}

impl<P: PlatformHooks + Clone> MonitorService<P, ReqwestApiClient> {
    pub fn setup(mut config: Config, platform: P) -> CoreResult<Self> {
        config.refresh_from_runtime_file()?;
        let api = ReqwestApiClient::new(&config)?;
        Self::setup_with_api(config, platform, api)
    }
}

impl<P: PlatformHooks + Clone, A: ApiTransport + Clone> MonitorService<P, A> {
    pub fn setup_with_api(mut config: Config, platform: P, api: A) -> CoreResult<Self> {
        config.refresh_from_runtime_file()?;
        let storage = FileStateStore::new(&config.state_dir)?;
        let state_file_path = config.state_dir.join("event_state.json");
        let obs_state = EventLoop::<P, A>::load_state(&state_file_path)?;

        let auth = Auth::load(&storage)?;

        let screenshot_obs = ScreenshotObserver::new(
            obs_state.screenshot,
            platform.clone(),
            ScreenshotConfig {
                screenshot_interval: config.screenshot_interval,
            },
        );
        let mut upload_obs = UploadObserver::new(
            obs_state.upload,
            api.clone(),
            UploadConfig {
                batch_interval: config.batch_interval,
            },
        );
        let lifecycle_obs = LifecycleObserver::new(obs_state.lifecycle);

        if let Some(creds) = &auth.device_credentials {
            upload_obs.upload_api.set_credentials(Some(creds.clone()));
        }

        let observers = Observers {
            screenshot: screenshot_obs,
            upload: upload_obs,
            lifecycle: lifecycle_obs,
        };
        let event_loop = EventLoop::new(state_file_path, observers);

        let is_authenticated = auth.is_authenticated();
        let device_id = auth.device_id().map(|s| s.to_string());

        let mut status = storage.load_status()?.unwrap_or(ServiceStatus {
            is_authenticated,
            is_running: true,
            device_id: device_id.clone(),
            last_loop_at_ms: None,
            pending_request_count: event_loop.observers.upload.state.pending_request_count(),
            lifecycle: crate::lifecycle::LifecycleStatus::for_platform(&config.platform_name),
        });
        status.is_running = true;
        status.is_authenticated = is_authenticated;
        status.device_id = device_id;
        status.lifecycle.capabilities =
            crate::lifecycle::LifecycleCapabilities::for_platform(&config.platform_name);

        let mut service = Self {
            config,
            platform,
            api,
            storage,
            status,
            auth,
            event_loop,
        };

        if !service.auth.is_authenticated() {
            service.event_loop.observers.screenshot.reset_schedule();
            service.event_loop.observers.upload.state.last_batch_at_ms = None;
        }
        service.persist_state()?;

        if service.auth.is_authenticated() {
            let _ = service.refresh_device_settings();
        }
        service.persist_state()?;
        Ok(service)
    }

    pub fn loop_iteration(&mut self) -> CoreResult<LoopOutcome> {
        self.ensure_running()?;
        self.refresh_runtime_config()?;

        self.auth = Auth::load(&self.storage)?;
        self.event_loop
            .observers
            .upload
            .upload_api
            .set_credentials(self.auth.device_credentials.clone());

        let now_ms = self.platform.get_time_utc_ms()?;
        self.status.last_loop_at_ms = Some(now_ms);

        if self.auth.is_authenticated() {
            if !self.event_loop.observers.upload.has_settings() {
                let _ = self.refresh_device_settings();
            }
            if self.auth.is_authenticated() {
                if let Err(err) = self.event_loop.iter(now_ms) {
                    log_error("loop iteration failed", Some(&err));
                }
            }
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

        if self.auth.is_authenticated() {
            self.event_loop.queue_event(Event::Shutdown);
            let now_ms = self.platform.get_time_utc_ms().unwrap_or(0);
            let _ = self.event_loop.iter(now_ms);
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
        if intent.as_ref().is_some_and(|i| i.role == role) {
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
        let now_ms = self.platform.get_time_utc_ms()?;
        let is_authenticated = self.auth.is_authenticated();
        self.storage.append_lifecycle_observation(&observation)?;
        self.event_loop.queue_event(Event::LifecycleObserved {
            observation,
            now_ms,
            is_authenticated,
        });
        self.event_loop.iter(now_ms)?;
        let transitions = self
            .event_loop
            .observers
            .lifecycle
            .state
            .last_transitions
            .clone();
        self.persist_state()?;
        Ok(transitions)
    }

    pub fn next_service_ping_due_at_ms(&self, role: ServiceRole) -> CoreResult<Option<i64>> {
        if !self.auth.is_authenticated() {
            return Ok(None);
        }
        let due_at_ms = self
            .event_loop
            .observers
            .lifecycle
            .state
            .service_last_pings
            .get(role.as_str())
            .map(|ping| ping.pinged_at_ms.saturating_add(SERVICE_PING_INTERVAL_MS))
            .unwrap_or(self.platform.get_time_utc_ms()?);
        Ok(Some(due_at_ms))
    }

    pub fn record_service_ping_if_due(
        &mut self,
        role: ServiceRole,
        detected_by: &str,
    ) -> CoreResult<bool> {
        if !self.auth.is_authenticated() {
            return Ok(false);
        }
        let now_ms = self.platform.get_time_utc_ms()?;
        let previous_ping = self
            .event_loop
            .observers
            .lifecycle
            .state
            .service_last_pings
            .get(role.as_str())
            .cloned();
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
        self.event_loop
            .observers
            .lifecycle
            .state
            .service_last_pings
            .insert(role.as_str().to_string(), ping_log);
        self.event_loop.persist()?;
        Ok(true)
    }

    pub fn send_log(&mut self, log: LogEntry) -> CoreResult<()> {
        self.ensure_running()?;
        self.event_loop
            .queue_event(Event::ImmediateUpload { entry: log });
        let now_ms = self.platform.get_time_utc_ms()?;
        let _ = self.event_loop.iter(now_ms);
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
        self.event_loop
            .queue_event(Event::BatchUpload { data: event });
        let now_ms = self.platform.get_time_utc_ms()?;
        let _ = self.event_loop.iter(now_ms);
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
        let processed = ImagePipeline.process(screenshot)?;
        let batch_event = prepare_screenshot_batch_event(processed, kind, risk, data)?;
        self.event_loop
            .queue_event(Event::ScreenshotCaptured { data: batch_event });
        if self.auth.is_authenticated() {
            let now_ms = self.platform.get_time_utc_ms()?;
            let _ = self.event_loop.iter(now_ms);
        }
        self.persist_state()
    }

    pub fn upload_pending_batch_now(&mut self) -> CoreResult<(usize, usize)> {
        self.ensure_running()?;
        if !self.auth.is_authenticated() {
            return Ok((0, 0));
        }
        let count = self
            .event_loop
            .observers
            .upload
            .state
            .pending_batch_events
            .len()
            .min(crate::events::MAX_BATCH_ITEMS_PER_UPLOAD);
        if count == 0 {
            self.persist_state()?;
            return Ok((0, 0));
        }
        let now_ms = self.platform.get_time_utc_ms()?;
        self.event_loop.observers.upload.force_upload_now(now_ms)?;
        let remaining = self
            .event_loop
            .observers
            .upload
            .state
            .pending_batch_events
            .len();
        self.persist_state()?;
        Ok((count, remaining))
    }

    pub fn login(&mut self, username: &str, password: &str) -> CoreResult<LoginStatus> {
        self.ensure_running()?;

        let access_token = self.api.login(username, password)?;
        let mut device = self.api.register_device(
            &access_token,
            &self.config.device_name,
            &self.config.platform_name,
        )?;

        self.event_loop
            .observers
            .lifecycle
            .state
            .service_stop_markers
            .clear();
        self.event_loop
            .observers
            .lifecycle
            .state
            .service_last_pings
            .clear();
        self.storage.clear_stop_intent()?;

        // Fetch settings, refreshing the device token locally if needed.
        let settings = self
            .api
            .get_device_settings(&device.access_token)
            .or_else(|err| {
                if err.is_unauthorized() {
                    let refreshed = self.api.refresh_device_token(&device.refresh_token)?;
                    device.access_token = refreshed.clone();
                    self.api.get_device_settings(&refreshed)
                } else {
                    Err(err)
                }
            })?;

        self.auth.set_login(access_token.clone(), device.clone());
        self.auth.persist(&self.storage)?;
        self.event_loop
            .observers
            .upload
            .upload_api
            .set_credentials(Some(device.clone()));
        self.event_loop
            .observers
            .upload
            .set_settings(Some(settings));
        self.event_loop.observers.upload.state.reset_for_login();
        self.event_loop.observers.screenshot.reset_schedule();

        self.status.is_authenticated = true;
        self.status.device_id = Some(device.device_id.clone());

        self.record_lifecycle_observation(LifecycleObservation::UserSessionChanged {
            state: UserSessionState::LoggedIn,
            origin: crate::lifecycle::LifecycleOrigin::UserRequested,
            detected_by: "core_login".to_string(),
        })?;

        Ok(LoginStatus {
            access_token,
            device: Some(device),
        })
    }

    pub fn logout(&mut self) -> CoreResult<()> {
        self.ensure_running()?;

        let was_authenticated = self.auth.is_authenticated();
        let now_ms = self.platform.get_time_utc_ms()?;

        if let Some(token) = self.auth.user_access_token.clone() {
            let _ = self.api.logout(&token);
        }

        // Clear stale pending events first, then run the lifecycle loop so
        // the logout alert fires into the now-clean queues.
        self.event_loop
            .observers
            .upload
            .upload_api
            .set_credentials(None);
        self.event_loop.observers.upload.set_settings(None);
        self.event_loop.observers.upload.state.reset_for_logout();
        self.event_loop.observers.screenshot.reset_schedule();

        if was_authenticated {
            let observation = LifecycleObservation::UserSessionChanged {
                state: UserSessionState::LoggedOut,
                origin: crate::lifecycle::LifecycleOrigin::UserRequested,
                detected_by: "core_logout".to_string(),
            };
            self.storage.append_lifecycle_observation(&observation)?;
            self.event_loop.dispatch(
                Event::LifecycleObserved {
                    observation,
                    now_ms,
                    is_authenticated: true,
                },
                now_ms,
            )?;
        }

        self.auth.clear();
        self.auth.persist(&self.storage)?;

        self.event_loop
            .observers
            .lifecycle
            .state
            .service_stop_markers
            .clear();
        self.event_loop
            .observers
            .lifecycle
            .state
            .service_last_pings
            .clear();
        self.storage.clear_stop_intent()?;
        self.status.is_authenticated = false;
        self.status.device_id = None;
        self.persist_state()
    }

    pub fn status(&self) -> CoreResult<ServiceStatus> {
        let mut status = self
            .storage
            .load_status()?
            .unwrap_or_else(|| self.status.clone());
        status.pending_request_count = self
            .event_loop
            .observers
            .upload
            .state
            .pending_request_count();
        status.lifecycle.capabilities =
            crate::lifecycle::LifecycleCapabilities::for_platform(&self.config.platform_name);
        Ok(status)
    }

    fn refresh_device_settings(&mut self) -> CoreResult<()> {
        let mut credentials = self
            .auth
            .device_credentials
            .clone()
            .ok_or(CoreError::NotAuthenticated)?;

        let result = match self.api.get_device_settings(&credentials.access_token) {
            Err(err) if err.is_unauthorized() => {
                let refreshed = self.api.refresh_device_token(&credentials.refresh_token)?;
                credentials.access_token = refreshed.clone();
                self.auth.set_credentials(credentials.clone());
                self.auth.persist(&self.storage)?;
                self.event_loop
                    .observers
                    .upload
                    .upload_api
                    .set_credentials(Some(credentials));
                self.api.get_device_settings(&refreshed)
            }
            other => other,
        };

        match result {
            Ok(settings) => {
                self.event_loop
                    .observers
                    .upload
                    .set_settings(Some(settings));
                Ok(())
            }
            Err(err) if err.is_not_found() => {
                log_error("device not found; clearing local auth", Some(&err));
                self.clear_auth()?;
                Err(CoreError::NotAuthenticated)
            }
            Err(err) => {
                log_error("device settings refresh failed", Some(&err));
                Err(err)
            }
        }
    }

    fn clear_auth(&mut self) -> CoreResult<()> {
        self.auth.clear();
        self.auth.persist(&self.storage)?;
        self.event_loop
            .observers
            .upload
            .upload_api
            .set_credentials(None);
        self.event_loop.observers.upload.set_settings(None);
        self.event_loop.observers.upload.state.reset_for_logout();
        self.event_loop.observers.screenshot.reset_schedule();
        self.event_loop
            .observers
            .lifecycle
            .state
            .service_stop_markers
            .clear();
        self.event_loop
            .observers
            .lifecycle
            .state
            .service_last_pings
            .clear();
        self.storage.clear_stop_intent()?;
        self.status.is_authenticated = false;
        self.status.device_id = None;
        self.persist_state()
    }

    fn persist_state(&mut self) -> CoreResult<()> {
        self.status.is_authenticated = self.auth.is_authenticated();
        self.status.device_id = self.auth.device_id().map(|s| s.to_string());
        self.status.pending_request_count = self
            .event_loop
            .observers
            .upload
            .state
            .pending_request_count();
        self.status.lifecycle = self.event_loop.observers.lifecycle.state.lifecycle.clone();
        self.status.lifecycle.capabilities =
            crate::lifecycle::LifecycleCapabilities::for_platform(&self.config.platform_name);

        self.storage.save_status(&self.status)?;
        self.event_loop.persist()?;
        Ok(())
    }

    fn refresh_runtime_config(&mut self) -> CoreResult<()> {
        let previous_base_url = self.config.api_base_url.clone();
        self.config.refresh_from_runtime_file()?;
        if self.config.api_base_url != previous_base_url {
            self.api.reconfigure(&self.config)?;
        }
        self.event_loop
            .observers
            .screenshot
            .config
            .screenshot_interval = self.config.screenshot_interval;
        self.event_loop.observers.upload.config.batch_interval = self.config.batch_interval;
        Ok(())
    }

    fn ensure_running(&self) -> CoreResult<()> {
        if self.status.is_running {
            Ok(())
        } else {
            Err(CoreError::Shutdown)
        }
    }

    fn next_run_at_ms(&self, now_ms: i64) -> i64 {
        if !self.auth.is_authenticated() {
            return now_ms + self.config.screenshot_interval.as_millis() as i64;
        }
        let screenshot_due = self
            .event_loop
            .observers
            .screenshot
            .state
            .last_screenshot_at_ms
            .map_or(
                now_ms + self.config.screenshot_interval.as_millis() as i64,
                |last| last + self.config.screenshot_interval.as_millis() as i64,
            );
        let batch_due = self
            .event_loop
            .observers
            .upload
            .state
            .last_batch_at_ms
            .map_or(
                now_ms + self.config.batch_interval.as_millis() as i64,
                |last| last + self.config.batch_interval.as_millis() as i64,
            );
        screenshot_due.min(batch_due)
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
    use crate::api::ReqwestApiClient;
    use crate::lifecycle::{CaptureAvailabilityState, CapturePermissionState, ComputerPowerState};
    use crate::model::{BatchRecipient, DeviceCredentials, DeviceSettings};

    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[derive(Clone)]
    struct TestPlatform {
        now_ms: Arc<AtomicI64>,
    }

    impl PlatformHooks for TestPlatform {
        fn take_screenshot(&self) -> CoreResult<crate::model::Screenshot> {
            Ok(crate::model::Screenshot {
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

    fn build_service(state_dir: PathBuf) -> MonitorService<TestPlatform, ReqwestApiClient> {
        let config = test_config(state_dir.clone());
        let platform = TestPlatform::new(0);
        let api = ReqwestApiClient::new(&config).expect("create api client");
        let mut service =
            MonitorService::setup_with_api(config, platform, api).expect("setup service");

        // Seed device settings directly in observer state
        service
            .event_loop
            .observers
            .upload
            .set_settings(Some(DeviceSettings {
                device_id: "device-1".to_string(),
                name: "Device".to_string(),
                platform: "test".to_string(),
                owner: Some(BatchRecipient {
                    user_id: "user-1".to_string(),
                    pub_key_base64: "owner-key".to_string(),
                }),
                partners: Vec::new(),
                hash_base_url: None,
            }));

        // Set timestamps to simulate state with stale last-run timestamps
        service
            .event_loop
            .observers
            .screenshot
            .state
            .last_screenshot_at_ms = Some(1000);
        service.event_loop.observers.upload.state.last_batch_at_ms = Some(1000);
        service
            .event_loop
            .observers
            .upload
            .state
            .post_login_proof_batches_remaining = 0;

        service
    }

    fn authenticate_service(service: &mut MonitorService<TestPlatform, ReqwestApiClient>) {
        let creds = DeviceCredentials {
            device_id: "device-1".to_string(),
            access_token: "device-access".to_string(),
            refresh_token: "device-refresh".to_string(),
        };
        service.auth.device_credentials = Some(creds.clone());
        service
            .event_loop
            .observers
            .upload
            .upload_api
            .set_credentials(Some(creds));
        service.status.is_authenticated = true;
        service.status.device_id = Some("device-1".to_string());
    }

    fn lifecycle_direct_logs(
        service: &MonitorService<TestPlatform, ReqwestApiClient>,
    ) -> Vec<LogEntry> {
        service
            .event_loop
            .observers
            .upload
            .state
            .pending_immediate_events
            .clone()
    }

    fn lifecycle_batch_logs(
        service: &MonitorService<TestPlatform, ReqwestApiClient>,
    ) -> Vec<LogEntry> {
        service
            .event_loop
            .observers
            .upload
            .state
            .pending_batch_events
            .iter()
            .map(|e| e.event.clone())
            .collect()
    }

    fn lifecycle_audit_logs(
        service: &MonitorService<TestPlatform, ReqwestApiClient>,
    ) -> Vec<LogEntry> {
        let mut logs = lifecycle_direct_logs(service);
        logs.extend(lifecycle_batch_logs(service));
        logs.sort_by_key(|log| log.ts);
        logs
    }

    fn service_ping_state(
        service: &MonitorService<TestPlatform, ReqwestApiClient>,
        role: ServiceRole,
    ) -> Option<ServicePingLog> {
        service
            .event_loop
            .observers
            .lifecycle
            .state
            .service_last_pings
            .get(role.as_str())
            .cloned()
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
                pending_request_count: 0,
                lifecycle: crate::lifecycle::LifecycleStatus::for_platform("test"),
            })
            .expect("save stale status");

        let service = MonitorService::setup(test_config(state_dir.clone()), TestPlatform::new(0))
            .expect("setup service");

        assert_eq!(
            service
                .event_loop
                .observers
                .screenshot
                .state
                .last_screenshot_at_ms,
            None
        );
        assert_eq!(
            service.event_loop.observers.upload.state.last_batch_at_ms,
            None
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn next_run_stays_in_future_when_logged_out_with_stale_timestamps() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());
        service
            .event_loop
            .observers
            .screenshot
            .state
            .last_screenshot_at_ms = Some(1);
        service.event_loop.observers.upload.state.last_batch_at_ms = Some(1);

        let next_run_at_ms = service.next_run_at_ms(10_000);

        assert_eq!(next_run_at_ms, 10_000 + 300_000);

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn status_derives_pending_request_count_from_observer_state() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());
        service
            .storage
            .save_status(&ServiceStatus {
                is_authenticated: true,
                is_running: true,
                device_id: Some("device-1".to_string()),
                last_loop_at_ms: Some(1),
                pending_request_count: 0,
                lifecycle: crate::lifecycle::LifecycleStatus::for_platform("test"),
            })
            .expect("save stale status");

        service
            .event_loop
            .observers
            .upload
            .state
            .pending_immediate_events
            .push(LogEntry {
                ts: 1,
                kind: "system_event".to_string(),
                risk: None,
                data: EventData::from_pairs([("event".to_string(), "test".to_string())]),
            });

        let status = service.status().expect("load status");
        assert_eq!(status.pending_request_count, 1);

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

        assert_eq!(
            service
                .event_loop
                .observers
                .upload
                .state
                .pending_hash_events
                .len(),
            0
        );
        assert_eq!(
            service
                .event_loop
                .observers
                .upload
                .state
                .pending_batch_events
                .len(),
            1
        );
        let batch_event = &service
            .event_loop
            .observers
            .upload
            .state
            .pending_batch_events[0];
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
            .event_loop
            .observers
            .lifecycle
            .state
            .service_stop_markers
            .get(ServiceRole::PrimaryService.as_str())
            .expect("persisted stop marker");
        assert_eq!(
            stop_marker.origin,
            crate::lifecycle::LifecycleOrigin::UserRequested
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn lifecycle_observations_emit_suspend_and_resume_transition_logs() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());

        service
            .record_lifecycle_observation(LifecycleObservation::ComputerPowerChanged {
                state: ComputerPowerState::Suspended,
                origin: crate::lifecycle::LifecycleOrigin::SystemSuspend,
                detected_by: "login1_prepare_for_sleep".to_string(),
                confidence: crate::lifecycle::LifecycleConfidence::Confirmed,
            })
            .expect("record suspend");
        service
            .record_lifecycle_observation(LifecycleObservation::ComputerPowerChanged {
                state: ComputerPowerState::Running,
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
                origin: crate::lifecycle::LifecycleOrigin::UserRequested,
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
        service.auth.user_access_token = Some("user-token".to_string());
        service
            .event_loop
            .observers
            .lifecycle
            .state
            .lifecycle
            .snapshot
            .user_session = UserSessionState::LoggedIn;

        // Simulate a stale pending event that should be cleared by logout.
        service
            .event_loop
            .observers
            .upload
            .state
            .pending_immediate_events
            .push(LogEntry {
                ts: 1,
                kind: "system_event".to_string(),
                risk: None,
                data: EventData::from_pairs([("event".to_string(), "stale".to_string())]),
            });

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

        // Stale event is gone; only the logout alert and transition remain.
        let all_logs = lifecycle_audit_logs(&service);
        assert!(all_logs.iter().all(|log| !(log.kind == "system_event"
            && log.data.get("event") == Some(&serde_json::Value::String("stale".to_string())))));
        assert!(!service.status.is_authenticated);

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn capture_permission_loss_emits_direct_high_risk_alert() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());
        authenticate_service(&mut service);
        service
            .event_loop
            .observers
            .lifecycle
            .state
            .lifecycle
            .snapshot
            .capture_permission = CapturePermissionState::Granted;

        service
            .record_lifecycle_observation(LifecycleObservation::CapturePermissionChanged {
                state: CapturePermissionState::Missing,
                detected_by: "macos_probe".to_string(),
            })
            .expect("record capture permission loss");

        let direct_logs = lifecycle_direct_logs(&service);
        let alert_log = direct_logs
            .iter()
            .find(|log| log.kind == "lifecycle_alert")
            .expect("direct lifecycle alert");
        assert_eq!(alert_log.risk, Some(0.9));
        assert_eq!(
            alert_log.data.get("alert_reason"),
            Some(&serde_json::Value::String(
                "capture_permission_changed".to_string()
            ))
        );
        assert_eq!(
            alert_log.data.get("from"),
            Some(&serde_json::Value::String("granted".to_string()))
        );
        assert_eq!(
            alert_log.data.get("to"),
            Some(&serde_json::Value::String("missing".to_string()))
        );

        let batch_logs = lifecycle_batch_logs(&service);
        assert_eq!(
            batch_logs
                .iter()
                .filter(|log| log.kind == "lifecycle_transition")
                .count(),
            1
        );
        let transition_log = batch_logs
            .iter()
            .find(|log| log.kind == "lifecycle_transition")
            .expect("batched lifecycle transition");
        assert_eq!(transition_log.risk, Some(0.0));
        assert!(
            batch_logs
                .iter()
                .all(|log| !(log.kind == "lifecycle_alert" && log.risk == Some(0.9)))
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn capture_state_changes_while_suspended_do_not_emit_alerts_or_transitions() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());
        authenticate_service(&mut service);
        service
            .event_loop
            .observers
            .lifecycle
            .state
            .lifecycle
            .snapshot
            .computer_power = ComputerPowerState::Suspended;
        service
            .event_loop
            .observers
            .lifecycle
            .state
            .lifecycle
            .snapshot
            .capture_permission = CapturePermissionState::Granted;
        service
            .event_loop
            .observers
            .lifecycle
            .state
            .lifecycle
            .snapshot
            .capture_availability = CaptureAvailabilityState::Ready;

        service
            .record_lifecycle_observation(LifecycleObservation::CapturePermissionChanged {
                state: CapturePermissionState::Missing,
                detected_by: "failed_loop".to_string(),
            })
            .expect("record capture permission during sleep");
        service
            .record_lifecycle_observation(LifecycleObservation::CaptureAvailabilityChanged {
                state: CaptureAvailabilityState::Blocked,
                detected_by: "failed_loop".to_string(),
            })
            .expect("record capture availability during sleep");

        assert!(lifecycle_direct_logs(&service).is_empty());
        assert!(
            lifecycle_batch_logs(&service)
                .iter()
                .all(|log| log.kind != "lifecycle_transition" && log.kind != "lifecycle_alert")
        );
        assert_eq!(
            service
                .event_loop
                .observers
                .lifecycle
                .state
                .lifecycle
                .snapshot
                .capture_permission,
            CapturePermissionState::Granted
        );
        assert_eq!(
            service
                .event_loop
                .observers
                .lifecycle
                .state
                .lifecycle
                .snapshot
                .capture_availability,
            CaptureAvailabilityState::Ready
        );

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn capture_permission_gain_emits_batched_low_risk_alert() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());
        authenticate_service(&mut service);
        service
            .event_loop
            .observers
            .lifecycle
            .state
            .lifecycle
            .snapshot
            .capture_permission = CapturePermissionState::Missing;

        service
            .record_lifecycle_observation(LifecycleObservation::CapturePermissionChanged {
                state: CapturePermissionState::Granted,
                detected_by: "macos_probe".to_string(),
            })
            .expect("record capture permission gain");

        assert!(
            lifecycle_direct_logs(&service)
                .iter()
                .all(|log| log.kind != "lifecycle_alert")
        );

        let batch_logs = lifecycle_batch_logs(&service);
        let alert_log = batch_logs
            .iter()
            .find(|log| log.kind == "lifecycle_alert")
            .expect("batched lifecycle alert");
        assert_eq!(alert_log.risk, Some(0.2));
        assert_eq!(
            alert_log.data.get("from"),
            Some(&serde_json::Value::String("missing".to_string()))
        );
        assert_eq!(
            alert_log.data.get("to"),
            Some(&serde_json::Value::String("granted".to_string()))
        );
        let transition_log = batch_logs
            .iter()
            .find(|log| log.kind == "lifecycle_transition")
            .expect("batched lifecycle transition");
        assert_eq!(transition_log.risk, Some(0.0));

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
                .event_loop
                .observers
                .lifecycle
                .state
                .service_stop_markers
                .get(ServiceRole::PrimaryService.as_str())
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
