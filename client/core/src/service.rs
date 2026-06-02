use std::time::Duration;

use crate::api::{ApiTransport, ReqwestApiClient};
use crate::auth::Auth;
use crate::config::Config;
use crate::error::{CoreError, CoreResult};
use crate::events::UploadKind;
use crate::events::{Event, EventLoop, log_error};
use crate::model::{LoginStatus, LoopOutcome, ServiceStatus, StopIntent, UserSessionState};
use crate::module::capture_availability::CaptureAvailabilityObserver;
use crate::module::lifecycle::LifecycleObserver;
use crate::module::screenshot::image_pipeline::ImagePipeline;
use crate::module::screenshot::{ScreenshotConfig, ScreenshotObserver};
use crate::module::upload::{UploadConfig, UploadObserver};
use crate::platform::PlatformHooks;
use crate::storage::FileStateStore;

pub const ITER_INTERVAL: Duration = Duration::from_secs(1);

pub async fn iter_sleep() {
    tokio::time::sleep(ITER_INTERVAL).await;
}

const SCREENSHOT_IDX: usize = 1;
const UPLOAD_IDX: usize = 2;

pub struct MonitorService<
    P: PlatformHooks + Clone + 'static,
    A: ApiTransport + Clone + 'static = ReqwestApiClient,
> {
    config: Config,
    platform: P,
    api: A,
    storage: FileStateStore,
    status: ServiceStatus,
    pub(crate) auth: Auth,
    pub(crate) event_loop: EventLoop,
}

impl<P: PlatformHooks + Clone + 'static> MonitorService<P, ReqwestApiClient> {
    pub fn setup(mut config: Config, platform: P) -> CoreResult<Self> {
        config.refresh_from_runtime_file()?;
        let api = ReqwestApiClient::new(&config)?;
        Self::setup_with_api(config, platform, api)
    }
}

impl<P: PlatformHooks + Clone + 'static, A: ApiTransport + Clone + 'static> MonitorService<P, A> {
    pub fn setup_with_api(mut config: Config, platform: P, api: A) -> CoreResult<Self> {
        config.refresh_from_runtime_file()?;
        let storage = FileStateStore::new(&config.state_dir)?;
        let state_file_path = config.state_dir.join("event_state.json");

        let auth = Auth::load(&storage)?;

        let mut event_loop = EventLoop::new(state_file_path.clone(), vec![]);
        let tx = event_loop.tx.clone();

        let lifecycle_obs = LifecycleObserver::new(Box::new(platform.clone()), tx.clone());
        let screenshot_obs = ScreenshotObserver::new(
            Box::new(platform.clone()),
            tx.clone(),
            ScreenshotConfig {
                screenshot_interval: config.screenshot_interval,
            },
        );
        let upload_obs = UploadObserver::new(
            Box::new(platform.clone()),
            api.clone(),
            UploadConfig {
                batch_interval: config.batch_interval,
            },
            auth.device_credentials.clone(),
        );
        let capture_availability_obs =
            CaptureAvailabilityObserver::new(tx.clone(), Box::new(platform.clone()));

        event_loop.observers = vec![
            Box::new(lifecycle_obs),
            Box::new(screenshot_obs),
            Box::new(upload_obs),
            Box::new(capture_availability_obs),
        ];
        event_loop.load_state(&state_file_path)?;

        let is_authenticated = auth.is_authenticated();
        let device_id = auth.device_id().map(|s| s.to_string());

        let pending_count = Self::upload_obs_in(&event_loop)
            .state
            .pending_request_count();
        let mut status = storage.load_status()?.unwrap_or(ServiceStatus {
            is_authenticated,
            is_running: true,
            device_id: device_id.clone(),
            last_loop_at_ms: None,
            pending_request_count: pending_count,
        });
        status.is_running = true;
        status.is_authenticated = is_authenticated;
        status.device_id = device_id;

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
            service.screenshot_obs_mut().reset_schedule();
            service.upload_obs_mut().state.last_batch_at_ms = None;
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
        let creds = self.auth.device_credentials.clone();
        self.upload_obs_mut().upload_api.set_credentials(creds);

        let now_ms = self.platform.get_time_utc_ms()?;
        self.status.last_loop_at_ms = Some(now_ms);

        if self.auth.is_authenticated() {
            if !self.upload_obs_mut().has_settings() {
                let _ = self.refresh_device_settings();
            }
            if self.auth.is_authenticated()
                && let Err(err) = self.event_loop.iter()
            {
                log_error("loop iteration failed", Some(&err));
            }
        }

        self.persist_state()?;

        Ok(LoopOutcome {
            ran_at_ms: now_ms,
            status: self.status.clone(),
        })
    }

    pub fn queue_event(&mut self, event: Event) {
        self.event_loop.queue_event(event);
    }

    pub fn run_event_loop_iter(&mut self) -> CoreResult<()> {
        if self.auth.is_authenticated() {
            let _ = self.event_loop.iter();
        }
        self.persist_state()
    }

    pub fn mark_stopped(&mut self) -> CoreResult<()> {
        self.status.is_running = false;
        self.persist_state()
    }

    pub fn note_stop_requested_by_user(&mut self, source: &str) -> CoreResult<()> {
        let requested_at_ms = self.platform.get_time_utc_ms()?;
        self.storage.save_stop_intent(&StopIntent {
            source: source.to_string(),
            requested_at_ms,
        })?;
        Ok(())
    }

    pub fn take_stop_intent(&mut self) -> CoreResult<Option<StopIntent>> {
        let intent = self.storage.load_stop_intent()?;
        if intent.is_some() {
            self.storage.clear_stop_intent()?;
        }
        Ok(intent)
    }

    pub fn send_log(&mut self, risk: f32, kind: UploadKind) -> CoreResult<()> {
        self.ensure_running()?;
        self.event_loop.queue_event(Event::Upload { risk, kind });
        let _ = self.event_loop.iter();
        self.persist_state()
    }

    pub fn queue_batch_log(&mut self, risk: f32, kind: UploadKind) -> CoreResult<()> {
        self.ensure_running()?;
        self.event_loop.queue_event(Event::Upload { risk, kind });
        let _ = self.event_loop.iter();
        self.persist_state()
    }

    pub fn capture_batch_screenshot(&mut self, risk: Option<f32>) -> CoreResult<()> {
        self.ensure_running()?;
        let screenshot = self.platform.take_screenshot()?;
        let processed = ImagePipeline.process(screenshot)?;
        self.event_loop.queue_event(Event::Upload {
            risk: risk.unwrap_or(0.0),
            kind: UploadKind::Screenshot {
                image: processed.bytes,
                content_type: processed.content_type,
            },
        });
        if self.auth.is_authenticated() {
            let _ = self.event_loop.iter();
        }
        self.persist_state()
    }

    pub fn upload_pending_batch_now(&mut self) -> CoreResult<(usize, usize)> {
        self.ensure_running()?;
        if !self.auth.is_authenticated() {
            return Ok((0, 0));
        }
        let count = self
            .upload_obs()
            .state
            .pending_batch_events
            .len()
            .min(crate::module::upload::MAX_BATCH_ITEMS_PER_UPLOAD);
        if count == 0 {
            self.persist_state()?;
            return Ok((0, 0));
        }
        let now_ms = self.platform.get_time_utc_ms()?;
        self.upload_obs_mut().force_upload_now(now_ms)?;
        let remaining = self.upload_obs().state.pending_batch_events.len();
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

        self.storage.clear_stop_intent()?;

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
        self.upload_obs_mut()
            .upload_api
            .set_credentials(Some(device.clone()));
        self.upload_obs_mut().set_settings(Some(settings));
        self.upload_obs_mut().state.reset_for_login();
        self.screenshot_obs_mut().reset_schedule();

        self.status.is_authenticated = true;
        self.status.device_id = Some(device.device_id.clone());

        self.event_loop
            .queue_event(Event::UserSessionChanged(UserSessionState::LoggedIn));
        let _ = self.event_loop.iter();
        self.persist_state()?;

        Ok(LoginStatus {
            access_token,
            device: Some(device),
        })
    }

    pub fn logout(&mut self) -> CoreResult<()> {
        self.ensure_running()?;

        let was_authenticated = self.auth.is_authenticated();

        if let Some(token) = self.auth.user_access_token.clone() {
            let _ = self.api.logout(&token);
        }

        self.upload_obs_mut().upload_api.set_credentials(None);
        self.upload_obs_mut().set_settings(None);
        self.upload_obs_mut().state.reset_for_logout();
        self.screenshot_obs_mut().reset_schedule();

        if was_authenticated {
            self.event_loop
                .queue_event(Event::UserSessionChanged(UserSessionState::LoggedOut));
            let _ = self.event_loop.iter();
        }

        self.auth.clear();
        self.auth.persist(&self.storage)?;

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
        status.pending_request_count = self.upload_obs().state.pending_request_count();
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
                self.upload_obs_mut()
                    .upload_api
                    .set_credentials(Some(credentials));
                self.api.get_device_settings(&refreshed)
            }
            other => other,
        };

        match result {
            Ok(settings) => {
                self.upload_obs_mut().set_settings(Some(settings));
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
        self.upload_obs_mut().upload_api.set_credentials(None);
        self.upload_obs_mut().set_settings(None);
        self.upload_obs_mut().state.reset_for_logout();
        self.screenshot_obs_mut().reset_schedule();
        self.storage.clear_stop_intent()?;
        self.status.is_authenticated = false;
        self.status.device_id = None;
        self.persist_state()
    }

    fn persist_state(&mut self) -> CoreResult<()> {
        self.status.is_authenticated = self.auth.is_authenticated();
        self.status.device_id = self.auth.device_id().map(|s| s.to_string());
        self.status.pending_request_count = self.upload_obs().state.pending_request_count();
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
        self.screenshot_obs_mut().config.screenshot_interval = self.config.screenshot_interval;
        self.upload_obs_mut().config.batch_interval = self.config.batch_interval;
        Ok(())
    }

    fn ensure_running(&self) -> CoreResult<()> {
        if self.status.is_running {
            Ok(())
        } else {
            Err(CoreError::Shutdown)
        }
    }

    // ─── Typed observer accessors ─────────────────────────────────────────────

    #[cfg_attr(not(test), allow(dead_code))]
    fn screenshot_obs(&self) -> &ScreenshotObserver {
        self.event_loop.observers[SCREENSHOT_IDX]
            .as_any()
            .downcast_ref::<ScreenshotObserver>()
            .expect("screenshot observer at index 1")
    }

    fn screenshot_obs_mut(&mut self) -> &mut ScreenshotObserver {
        self.event_loop.observers[SCREENSHOT_IDX]
            .as_any_mut()
            .downcast_mut::<ScreenshotObserver>()
            .expect("screenshot observer at index 1")
    }

    pub(crate) fn upload_obs(&self) -> &UploadObserver<A> {
        self.event_loop.observers[UPLOAD_IDX]
            .as_any()
            .downcast_ref::<UploadObserver<A>>()
            .expect("upload observer at index 2")
    }

    pub(crate) fn upload_obs_mut(&mut self) -> &mut UploadObserver<A> {
        self.event_loop.observers[UPLOAD_IDX]
            .as_any_mut()
            .downcast_mut::<UploadObserver<A>>()
            .expect("upload observer at index 2")
    }

    // Static versions for use before self is fully constructed
    fn upload_obs_in(event_loop: &EventLoop) -> &UploadObserver<A> {
        event_loop.observers[UPLOAD_IDX]
            .as_any()
            .downcast_ref::<UploadObserver<A>>()
            .expect("upload observer at index 2")
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
    use crate::events::UploadKind;
    use crate::model::{BatchRecipient, DeviceCredentials, DeviceSettings, LogEntry};

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

        fn get_last_shutdown_time_utc_ms(&self) -> CoreResult<Option<i64>> {
            Ok(None)
        }

        fn get_last_startup_time_utc_ms(&self) -> CoreResult<Option<i64>> {
            Ok(None)
        }
    }

    impl TestPlatform {
        fn new(now_ms: i64) -> Self {
            Self {
                now_ms: Arc::new(AtomicI64::new(now_ms)),
            }
        }

        #[allow(dead_code)]
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

        service.upload_obs_mut().set_settings(Some(DeviceSettings {
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

        service.screenshot_obs_mut().state.last_screenshot_at_ms = Some(1000);
        service.upload_obs_mut().state.last_batch_at_ms = Some(1000);
        service
            .upload_obs_mut()
            .state
            .post_login_proof_batches_remaining = 0;

        service
    }

    #[allow(dead_code)]
    fn authenticate_service(service: &mut MonitorService<TestPlatform, ReqwestApiClient>) {
        let creds = DeviceCredentials {
            device_id: "device-1".to_string(),
            access_token: "device-access".to_string(),
            refresh_token: "device-refresh".to_string(),
        };
        service.auth.device_credentials = Some(creds.clone());
        service
            .upload_obs_mut()
            .upload_api
            .set_credentials(Some(creds));
        service.status.is_authenticated = true;
        service.status.device_id = Some("device-1".to_string());
    }

    #[allow(dead_code)]
    fn pending_immediate_events(
        service: &MonitorService<TestPlatform, ReqwestApiClient>,
    ) -> Vec<LogEntry> {
        service.upload_obs().state.pending_immediate_events.clone()
    }

    #[allow(dead_code)]
    fn pending_batch_events(
        service: &MonitorService<TestPlatform, ReqwestApiClient>,
    ) -> Vec<LogEntry> {
        service
            .upload_obs()
            .state
            .pending_batch_events
            .iter()
            .map(|(_, bytes)| {
                rmp_serde::from_slice::<LogEntry>(bytes)
                    .expect("decode log entry from batch events")
            })
            .collect()
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
            })
            .expect("save stale status");

        let service = MonitorService::setup(test_config(state_dir.clone()), TestPlatform::new(0))
            .expect("setup service");

        assert_eq!(service.screenshot_obs().state.last_screenshot_at_ms, None);
        assert_eq!(service.upload_obs().state.last_batch_at_ms, None);

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
            })
            .expect("save stale status");

        service
            .upload_obs_mut()
            .state
            .pending_immediate_events
            .push(LogEntry {
                ts: 1,
                risk: None,
                event: UploadKind::Alert {
                    message: "test".to_string(),
                },
            });

        let status = service.status().expect("load status");
        assert_eq!(status.pending_request_count, 1);

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn queue_batch_log_creates_pending_hash_event() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());

        service
            .queue_batch_log(
                0.7,
                UploadKind::Dev {
                    title: "Developer test".to_string(),
                    details: None,
                },
            )
            .expect("queue batch log");

        // Without credentials, the hash upload defers — event stays in pending_hash_events.
        assert_eq!(service.upload_obs().state.pending_batch_events.len(), 0);
        assert_eq!(service.upload_obs().state.pending_hash_events.len(), 1);
        let entry = &service.upload_obs().state.pending_hash_events[0];
        assert!(matches!(&entry.event, UploadKind::Dev { title, .. } if title == "Developer test"));
        assert_eq!(entry.risk, Some(0.7));

        let _ = fs::remove_dir_all(state_dir);
    }
}
