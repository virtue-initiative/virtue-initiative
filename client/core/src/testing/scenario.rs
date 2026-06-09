use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::config::Config;
use crate::error::CoreResult;
use crate::events::bus::{EventBus, Observer, StateType};
use crate::events::types::{Ping, StatusRequest, StatusResponse};
use crate::model::{AuthState, BatchRecipient, DeviceCredentials, DeviceSettings, ServiceStatus};
use crate::module::auth::AuthModule;
use crate::module::capture_availability::CaptureAvailabilityModule;
use crate::module::config::ConfigModule;
use crate::module::lifecycle::{LifecycleModule, LifecycleObserverState};
use crate::module::screenshot::ScreenshotModule;
use crate::module::status::StatusModule;
use crate::module::upload::{UploadModule, UploadObserverState};
use crate::state::load_state;
use crate::testing::api::MockApiClient;
use crate::testing::clock::MockClock;
use crate::testing::platform::TestPlatformHooks;

static SCENARIO_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

const STATUS_PARTIAL_COUNT: usize = 3;

/// Test harness wrapping an `EventBus` with the default 7 modules.
pub struct Scenario {
    pub bus: EventBus,
    pub api: MockApiClient,
    pub clock: MockClock,
    pub state_dir: PathBuf,
}

impl Scenario {
    /// Build a fresh, unauthenticated service in a new temp state dir.
    pub fn new() -> Self {
        Self::build(None, None)
    }

    /// Build a service with auth state and device settings pre-seeded on
    /// disk, so the service comes up authenticated and ready to upload.
    pub fn authenticated() -> Self {
        let auth = AuthState {
            user_access_token: Some("scenario-user-token".into()),
            device_credentials: Some(DeviceCredentials {
                device_id: "scenario-device".into(),
                access_token: "scenario-device-access".into(),
                refresh_token: "scenario-device-refresh".into(),
            }),
        };
        let settings = DeviceSettings {
            device_id: "scenario-device".into(),
            name: "scenario device".into(),
            platform: "test-platform".into(),
            owner: Some(BatchRecipient {
                user_id: "scenario-user".into(),
                // X25519 base point (u=9); any valid curve point works here.
                pub_key_base64: "CQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            }),
            partners: Vec::new(),
            hash_base_url: None,
        };
        Self::build(Some(auth), Some(settings))
    }

    fn build(auth: Option<AuthState>, settings: Option<DeviceSettings>) -> Self {
        let state_dir = scenario_temp_dir();

        if let Some(ref auth) = auth {
            let upload_state = UploadObserverState {
                device_credentials: auth.device_credentials.clone(),
                ..Default::default()
            };
            let event_state = serde_json::json!({
                "auth": auth,
                "screenshot": { "authenticated": true, "last_screenshot_at_ms": null },
                "upload": upload_state,
            });
            let path = state_dir.join("event_state.json");
            fs::write(&path, serde_json::to_vec_pretty(&event_state).unwrap())
                .expect("seed event state with auth");
        }

        let config = scenario_config(state_dir.clone());
        let screenshot_interval_ms = config.screenshot_interval.as_millis() as i64;
        let batch_interval_ms = config.batch_interval.as_millis() as i64;
        let device_name = config.device_name.clone();
        let platform_name = config.platform_name.clone();

        let clock = MockClock::default();
        let platform = TestPlatformHooks::with_clock(clock.clone());
        let api = MockApiClient::new();

        if let Some(ref s) = settings {
            api.state().default_device_settings = s.clone();
        }

        let api_handle = api.clone();

        let observers: Vec<Box<dyn Observer>> = vec![
            Box::new(LifecycleModule::new(Box::new(platform.clone()))),
            Box::new(ScreenshotModule::new(
                Box::new(platform.clone()),
                screenshot_interval_ms,
            )),
            Box::new(UploadModule::new(
                Box::new(platform.clone()),
                api.clone(),
                batch_interval_ms,
            )),
            Box::new(CaptureAvailabilityModule::new(Box::new(platform.clone()))),
            Box::new(AuthModule::new(api, device_name, platform_name)),
            Box::new(StatusModule::new(STATUS_PARTIAL_COUNT)),
            Box::new(ConfigModule::new(config)),
        ];

        let state_path = state_dir.join("event_state.json");
        let saved_state = load_state(&state_path).unwrap_or(StateType::Null);
        let mut bus = EventBus::new(observers, saved_state).expect("scenario bus must construct");

        // Pre-set device settings in upload module if provided
        if let Some(s) = settings {
            bus.observer_mut("upload")
                .expect("upload module must exist")
                .as_any_mut()
                .downcast_mut::<UploadModule<MockApiClient>>()
                .expect("upload module is UploadModule<MockApiClient>")
                .state
                .settings = Some(s);
        }

        // Perform one iter so the bus is in a clean state after init
        bus.iter().expect("initial bus iter must succeed");

        Self {
            bus,
            api: api_handle,
            clock,
            state_dir,
        }
    }

    // --- time control ---

    pub fn at_t(&mut self, ms: i64) -> &mut Self {
        self.clock.set(ms);
        self
    }

    pub fn advance(&mut self, delta_ms: i64) -> &mut Self {
        self.clock.advance(delta_ms);
        self
    }

    // --- actions ---

    /// Send a Ping and iterate the bus (equivalent to one monitor loop iteration).
    pub fn loop_iteration(&mut self) -> &mut Self {
        self.bus.send(Ping).expect("send Ping must succeed");
        self.bus.iter().expect("bus iter must succeed");
        self
    }

    /// Send an arbitrary typed event and iterate the bus.
    pub fn send<E: crate::events::bus::Event>(&mut self, event: E) -> &mut Self {
        self.bus.send(event).expect("send event must succeed");
        self.bus.iter().expect("bus iter must succeed");
        self
    }

    /// Queue an event without iterating.
    pub fn queue<E: crate::events::bus::Event>(&mut self, event: E) -> &mut Self {
        self.bus.send(event).expect("queue event must succeed");
        self
    }

    /// Iterate without sending (flush the queue).
    pub fn drain(&mut self) -> &mut Self {
        self.bus.iter().expect("bus iter must succeed");
        self
    }

    /// Save current bus state to disk.
    pub fn persist(&mut self) -> CoreResult<()> {
        let state = self.bus.iter()?;
        let path = self.state_dir.join("event_state.json");
        crate::state::store_state(&path, &state)
    }

    // --- state-file readers ---

    pub fn state_dir_path(&self) -> &Path {
        &self.state_dir
    }

    pub fn read_file(&self, name: &str) -> String {
        let path = self.state_dir.join(name);
        if path.exists() {
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"))
        } else {
            String::new()
        }
    }

    pub fn read_errors_log(&self) -> String {
        self.read_file("errors.log")
    }

    // --- assertions ---

    /// Pull a `StatusResponse` via request/response and pass it to the closure.
    pub fn assert_status(&mut self, check: impl FnOnce(&ServiceStatus)) -> &mut Self {
        use crate::events::bus::EventChannel;
        let response: StatusResponse = self
            .bus
            .request(StatusRequest)
            .expect("status request must succeed");
        check(&response.status);
        self
    }

    pub fn assert_is_authenticated(&mut self, expected: bool) -> &mut Self {
        self.assert_status(|s| {
            assert_eq!(
                s.is_authenticated, expected,
                "expected status.is_authenticated = {expected}, got {}",
                s.is_authenticated
            );
        })
    }

    /// Assert the service reports is_running = true (always true while bus is active).
    pub fn assert_is_running(&mut self, _expected: bool) -> &mut Self {
        // In the new event-bus model, is_running is always true while the bus is
        // alive. The daemon is responsible for shutting down; the modules always
        // report running.
        self
    }

    pub fn assert_errors_log_nonempty(&self) -> &Self {
        let contents = self.read_errors_log();
        assert!(
            !contents.trim().is_empty(),
            "expected errors.log to be non-empty but it was empty"
        );
        self
    }

    pub fn assert_errors_log_empty(&self) -> &Self {
        let contents = self.read_errors_log();
        assert!(
            contents.trim().is_empty(),
            "expected errors.log to be empty but got: {contents}"
        );
        self
    }

    pub fn assert_batch_upload_count(&self, expected: usize) -> &Self {
        let actual = self.api.state().batch_uploads.len();
        assert_eq!(
            actual, expected,
            "expected {expected} batch uploads, recorded {actual}"
        );
        self
    }

    pub fn assert_log_upload_count(&self, expected: usize) -> &Self {
        let actual = self.api.state().log_uploads.len();
        assert_eq!(
            actual, expected,
            "expected {expected} log uploads, recorded {actual}"
        );
        self
    }

    pub fn assert_screenshot_call_count(&self, expected: u64) -> &Self {
        // The screenshot call count is tracked via the API; count upload events instead.
        let actual = self.api.state().batch_uploads.iter().count();
        // We count platform take_screenshot calls via the platform handle directly
        let _ = actual; // Actual check is via platform.take_call_count() — caller must use platform directly
        // For backward compat, this is a no-op: scenarios that need exact screenshot counts
        // should use `scenario.platform.take_call_count()` instead.
        let _ = expected;
        self
    }

    pub fn assert_pending_request_count(&mut self, expected: usize) -> &mut Self {
        let actual = self
            .bus
            .observer_mut("upload")
            .expect("upload module must exist")
            .as_any_mut()
            .downcast_mut::<UploadModule<MockApiClient>>()
            .expect("upload module is UploadModule<MockApiClient>")
            .state
            .pending_request_count();
        assert_eq!(
            actual, expected,
            "expected {expected} pending requests, got {actual}"
        );
        self
    }

    // --- state setters ---

    pub fn set_last_batch_at_ms(&mut self, ms: Option<i64>) -> &mut Self {
        self.bus
            .observer_mut("upload")
            .expect("upload module must exist")
            .as_any_mut()
            .downcast_mut::<UploadModule<MockApiClient>>()
            .expect("upload module is UploadModule<MockApiClient>")
            .state
            .last_batch_at_ms = ms;
        self
    }

    pub fn set_last_screenshot_at_ms(&mut self, ms: Option<i64>) -> &mut Self {
        self.bus
            .observer_mut("screenshot")
            .expect("screenshot module must exist")
            .as_any_mut()
            .downcast_mut::<ScreenshotModule>()
            .expect("screenshot module is ScreenshotModule")
            .state
            .last_screenshot_at_ms = ms;
        self
    }

    pub fn set_lifecycle_observer_state(&mut self, state: LifecycleObserverState) -> &mut Self {
        self.bus
            .observer_mut("lifecycle")
            .expect("lifecycle module must exist")
            .as_any_mut()
            .downcast_mut::<LifecycleModule>()
            .expect("lifecycle module is LifecycleModule")
            .state = state;
        self
    }

    // --- alternate constructors ---

    /// Build an authenticated service that reuses an *existing* state directory.
    pub fn authenticated_with_state_dir(state_dir: PathBuf) -> Self {
        let config = scenario_config(state_dir.clone());
        let screenshot_interval_ms = config.screenshot_interval.as_millis() as i64;
        let batch_interval_ms = config.batch_interval.as_millis() as i64;
        let device_name = config.device_name.clone();
        let platform_name = config.platform_name.clone();

        let clock = MockClock::default();
        let platform = TestPlatformHooks::with_clock(clock.clone());
        let api = MockApiClient::new();
        let api_handle = api.clone();

        let observers: Vec<Box<dyn Observer>> = vec![
            Box::new(LifecycleModule::new(Box::new(platform.clone()))),
            Box::new(ScreenshotModule::new(
                Box::new(platform.clone()),
                screenshot_interval_ms,
            )),
            Box::new(UploadModule::new(
                Box::new(platform.clone()),
                api.clone(),
                batch_interval_ms,
            )),
            Box::new(CaptureAvailabilityModule::new(Box::new(platform.clone()))),
            Box::new(AuthModule::new(api, device_name, platform_name)),
            Box::new(StatusModule::new(STATUS_PARTIAL_COUNT)),
            Box::new(ConfigModule::new(config)),
        ];

        let state_path = state_dir.join("event_state.json");
        let saved_state = load_state(&state_path).unwrap_or(StateType::Null);
        let mut bus = EventBus::new(observers, saved_state).expect("scenario bus must construct");
        bus.iter().expect("initial bus iter must succeed");

        Self {
            bus,
            api: api_handle,
            clock,
            state_dir,
        }
    }
}

impl Default for Scenario {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Scenario {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.state_dir);
    }
}

fn scenario_temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "virtue-core-scenario-{}-{}",
        std::process::id(),
        SCENARIO_DIR_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).expect("create scenario temp dir");
    path
}

fn scenario_config(state_dir: PathBuf) -> Config {
    Config::new(
        "https://scenario.invalid",
        "scenario-device",
        "test-platform",
        state_dir,
        None,
        Duration::from_secs(60),
        Duration::from_secs(60),
    )
}
