use std::path::Path;
use std::time::Duration;

use crate::api::{ApiTransport, ReqwestApiClient};
use crate::config::Config;
use crate::error::{CoreError, CoreResult};
use crate::events::UploadKind;
use crate::events::{Event, EventLoop, Observer, log_error};
use crate::ipc::{IpcError, IpcListener, IpcSender};
use crate::model::{LoopOutcome, ServiceStatus};
use crate::module::auth::AuthObserver;
use crate::module::capture_availability::CaptureAvailabilityObserver;
use crate::module::lifecycle::LifecycleObserver;
use crate::module::request_handler::RequestObserver;
use crate::module::screenshot::image_pipeline::ImagePipeline;
use crate::module::screenshot::{ScreenshotConfig, ScreenshotObserver};
use crate::module::status::StatusObserver;
use crate::module::upload::{UploadConfig, UploadObserver};
use crate::platform::PlatformHooks;

pub const ITER_INTERVAL: Duration = Duration::from_secs(1);

pub async fn iter_sleep() {
    tokio::time::sleep(ITER_INTERVAL).await;
}

const LIFECYCLE_IDX: usize = 0;
const SCREENSHOT_IDX: usize = 1;
const UPLOAD_IDX: usize = 2;
const REQUEST_HANDLER_IDX: usize = 4;
const AUTH_IDX: usize = 5;

/// Number of observers that emit a `PartialStatus` in reply to a
/// `StatusRequest`: `AuthObserver`, `LifecycleObserver`, and `UploadObserver`.
const STATUS_PARTIAL_COUNT: usize = 3;

pub struct MonitorService<
    P: PlatformHooks + Clone + 'static,
    A: ApiTransport + Clone + 'static = ReqwestApiClient,
> {
    config: Config,
    platform: P,
    pub(crate) is_running: bool,
    pub(crate) event_loop: EventLoop<P::CustomEvent>,
    _phantom: std::marker::PhantomData<A>,
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
        let state_file_path = config.state_dir.join("event_state.json");

        let mut event_loop: EventLoop<P::CustomEvent> =
            EventLoop::new(state_file_path.clone(), vec![]);
        let tx = event_loop.tx.clone();

        let lifecycle_obs =
            LifecycleObserver::<P::CustomEvent>::new(Box::new(platform.clone()), tx.clone());
        let screenshot_obs = ScreenshotObserver::<P::CustomEvent>::new(
            Box::new(platform.clone()),
            tx.clone(),
            ScreenshotConfig {
                screenshot_interval: config.screenshot_interval,
            },
        );
        let upload_obs = UploadObserver::<A, P::CustomEvent>::new(
            Box::new(platform.clone()),
            api.clone(),
            UploadConfig {
                batch_interval: config.batch_interval,
            },
            tx.clone(),
        );
        let capture_availability_obs = CaptureAvailabilityObserver::<P::CustomEvent>::new(
            tx.clone(),
            Box::new(platform.clone()),
        );
        let request_handler = RequestObserver::new();
        let auth_obs = AuthObserver::<A, P::CustomEvent>::new(
            api,
            config.device_name.clone(),
            config.platform_name.clone(),
            tx.clone(),
        );
        let status_obs = StatusObserver::<P::CustomEvent>::new(STATUS_PARTIAL_COUNT, tx.clone());

        event_loop.observers = vec![
            Box::new(lifecycle_obs) as Box<dyn Observer<P::CustomEvent>>, // 0
            Box::new(screenshot_obs),                                     // 1
            Box::new(upload_obs),                                         // 2
            Box::new(capture_availability_obs),                           // 3
            Box::new(request_handler),                                    // 4
            Box::new(auth_obs),                                           // 5
            Box::new(status_obs),                                         // 6
        ];
        event_loop.load_state(&state_file_path)?;

        let service = Self {
            config,
            platform,
            is_running: true,
            event_loop,
            _phantom: std::marker::PhantomData,
        };

        Ok(service)
    }

    pub fn loop_iteration(&mut self) -> CoreResult<LoopOutcome> {
        self.ensure_running()?;
        self.refresh_runtime_config()?;

        // Process IPC requests without triggering screenshot capture.
        if let Err(err) = self.event_loop.drain_for_ipc() {
            log_error("ipc event drain failed", Some(&err));
        }

        if let Err(err) = self.event_loop.iter() {
            log_error("loop iteration failed", Some(&err));
        }

        let status = self.current_status();
        Ok(LoopOutcome {
            ran_at_ms: status.last_loop_at_ms.unwrap_or(0),
            status,
        })
    }

    pub fn add_observer(&mut self, observer: Box<dyn Observer<P::CustomEvent>>) {
        self.event_loop.observers.push(observer);
    }

    pub fn queue_event(&mut self, event: Event<P::CustomEvent>) {
        self.event_loop.queue_event(event);
    }

    pub fn run_event_loop_iter(&mut self) -> CoreResult<()> {
        self.event_loop.iter()
    }

    pub fn mark_stopped(&mut self) -> CoreResult<()> {
        self.is_running = false;
        self.event_loop.persist()
    }

    pub fn consume_user_stop_request(&mut self) -> bool {
        let _ = self.event_loop.drain_for_ipc();
        self.lifecycle_obs_mut().take_user_stop_requested()
    }

    pub fn send_log(&mut self, risk: f32, kind: UploadKind) -> CoreResult<()> {
        self.ensure_running()?;
        self.event_loop.queue_event(Event::Upload { risk, kind });
        let _ = self.event_loop.iter();
        self.event_loop.persist()
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
        let _ = self.event_loop.iter();
        self.event_loop.persist()
    }

    pub fn upload_pending_batch_now(&mut self) -> CoreResult<(usize, usize)> {
        self.ensure_running()?;
        if self.auth_obs().state.device_credentials.is_none() {
            return Ok((0, 0));
        }
        let count = self
            .upload_obs()
            .state
            .pending_batch_events
            .len()
            .min(crate::module::upload::MAX_BATCH_ITEMS_PER_UPLOAD);
        if count == 0 {
            self.event_loop.persist()?;
            return Ok((0, 0));
        }
        let now_ms = self.platform.get_time_utc_ms()?;
        self.upload_obs_mut().force_upload_now(now_ms)?;
        let remaining = self.upload_obs().state.pending_batch_events.len();
        self.event_loop.persist()?;
        Ok((count, remaining))
    }

    /// Assemble the current status directly from the observers that own each
    /// field. Mirrors what the `StatusObserver` builds from `PartialStatus`
    /// fragments over IPC, but for in-process callers.
    pub fn current_status(&self) -> ServiceStatus {
        let credentials = &self.auth_obs().state.device_credentials;
        let last_ping = self.lifecycle_obs().state.last_ping;
        ServiceStatus {
            is_authenticated: credentials.is_some(),
            is_running: self.is_running,
            device_id: credentials.as_ref().map(|c| c.device_id.clone()),
            last_loop_at_ms: (last_ping > 0).then_some(last_ping),
            pending_request_count: self.upload_obs().state.pending_request_count(),
        }
    }

    fn refresh_runtime_config(&mut self) -> CoreResult<()> {
        let previous_base_url = self.config.api_base_url.clone();
        self.config.refresh_from_runtime_file()?;
        if self.config.api_base_url != previous_base_url {
            let config = self.config.clone();
            self.auth_obs_mut().api.reconfigure(&config)?;
        }
        self.screenshot_obs_mut().config.screenshot_interval = self.config.screenshot_interval;
        self.upload_obs_mut().config.batch_interval = self.config.batch_interval;
        Ok(())
    }

    fn ensure_running(&self) -> CoreResult<()> {
        if self.is_running {
            Ok(())
        } else {
            Err(CoreError::Shutdown)
        }
    }

    // ─── IPC ──────────────────────────────────────────────────────────────────

    /// Bind the daemon's IPC listener at the given socket path.
    pub fn bind_ipc(&self, path: &Path) -> Result<IpcListener, IpcError> {
        IpcListener::bind(path)
    }

    /// Register a new controller connection with the `RequestObserver` so it
    /// receives event broadcasts.
    pub fn add_ipc_client(&mut self, sender: IpcSender) {
        self.request_handler_mut().add_client(sender);
    }

    /// Clone the event-loop sender so a receiver thread can forward inbound
    /// IPC events into the daemon's event queue.
    pub fn event_queue_sender(&self) -> std::sync::mpsc::Sender<Event<P::CustomEvent>> {
        self.event_loop.tx.clone()
    }

    // ─── Typed observer accessors ─────────────────────────────────────────────

    fn lifecycle_obs(&self) -> &LifecycleObserver<P::CustomEvent> {
        self.event_loop.observers[LIFECYCLE_IDX]
            .as_any()
            .downcast_ref::<LifecycleObserver<P::CustomEvent>>()
            .expect("lifecycle observer at index 0")
    }

    fn lifecycle_obs_mut(&mut self) -> &mut LifecycleObserver<P::CustomEvent> {
        self.event_loop.observers[LIFECYCLE_IDX]
            .as_any_mut()
            .downcast_mut::<LifecycleObserver<P::CustomEvent>>()
            .expect("lifecycle observer at index 0")
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn screenshot_obs(&self) -> &ScreenshotObserver<P::CustomEvent> {
        self.event_loop.observers[SCREENSHOT_IDX]
            .as_any()
            .downcast_ref::<ScreenshotObserver<P::CustomEvent>>()
            .expect("screenshot observer at index 1")
    }

    fn screenshot_obs_mut(&mut self) -> &mut ScreenshotObserver<P::CustomEvent> {
        self.event_loop.observers[SCREENSHOT_IDX]
            .as_any_mut()
            .downcast_mut::<ScreenshotObserver<P::CustomEvent>>()
            .expect("screenshot observer at index 1")
    }

    pub(crate) fn upload_obs(&self) -> &UploadObserver<A, P::CustomEvent> {
        self.event_loop.observers[UPLOAD_IDX]
            .as_any()
            .downcast_ref::<UploadObserver<A, P::CustomEvent>>()
            .expect("upload observer at index 2")
    }

    pub(crate) fn upload_obs_mut(&mut self) -> &mut UploadObserver<A, P::CustomEvent> {
        self.event_loop.observers[UPLOAD_IDX]
            .as_any_mut()
            .downcast_mut::<UploadObserver<A, P::CustomEvent>>()
            .expect("upload observer at index 2")
    }

    fn request_handler_mut(&mut self) -> &mut RequestObserver {
        self.event_loop.observers[REQUEST_HANDLER_IDX]
            .as_any_mut()
            .downcast_mut::<RequestObserver>()
            .expect("request handler observer at index 4")
    }

    fn auth_obs(&self) -> &AuthObserver<A, P::CustomEvent> {
        self.event_loop.observers[AUTH_IDX]
            .as_any()
            .downcast_ref::<AuthObserver<A, P::CustomEvent>>()
            .expect("auth observer at index 5")
    }

    fn auth_obs_mut(&mut self) -> &mut AuthObserver<A, P::CustomEvent> {
        self.event_loop.observers[AUTH_IDX]
            .as_any_mut()
            .downcast_mut::<AuthObserver<A, P::CustomEvent>>()
            .expect("auth observer at index 5")
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
    use crate::platform::ScreenshotHooks;

    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[derive(Clone)]
    struct TestPlatform {
        now_ms: Arc<AtomicI64>,
    }

    impl ScreenshotHooks for TestPlatform {
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

    impl PlatformHooks for TestPlatform {
        type CustomEvent = ();
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
        service.auth_obs_mut().state.device_credentials = Some(creds.clone());
        service
            .upload_obs_mut()
            .upload_api
            .set_credentials(Some(creds));
        service.upload_obs_mut().authenticated = true;
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

        // No auth.json = not authenticated; ScreenshotObserver starts unauthenticated
        // and clears any stale schedule on load_state.
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

        let status = service.current_status();
        assert_eq!(status.pending_request_count, 1);

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn send_log_creates_pending_hash_event() {
        let state_dir = temp_state_dir();
        let mut service = build_service(state_dir.clone());

        // Authenticate so that Upload events are not dropped by UploadObserver.
        authenticate_service(&mut service);

        service
            .send_log(
                0.7,
                UploadKind::Dev {
                    title: "Developer test".to_string(),
                    details: None,
                },
            )
            .expect("queue batch log");

        // Without credentials reaching the network, the hash upload defers — event stays in pending_hash_events.
        assert_eq!(service.upload_obs().state.pending_batch_events.len(), 0);
        assert_eq!(service.upload_obs().state.pending_hash_events.len(), 1);
        let entry = &service.upload_obs().state.pending_hash_events[0];
        assert!(matches!(&entry.event, UploadKind::Dev { title, .. } if title == "Developer test"));
        assert_eq!(entry.risk, Some(0.7));

        let _ = fs::remove_dir_all(state_dir);
    }
}
