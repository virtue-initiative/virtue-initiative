use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::config::Config;
use crate::error::CoreResult;
use crate::events::{Event, ProcessStoppedReason};
use crate::model::{AuthState, BatchRecipient, DeviceCredentials, DeviceSettings, ServiceStatus};
use crate::module::lifecycle::{LifecycleObserver, LifecycleObserverState};
use crate::module::screenshot::ScreenshotObserver;
use crate::service::MonitorService;
use crate::storage::FileStateStore;
use crate::testing::api::MockApiClient;
use crate::testing::clock::MockClock;
use crate::testing::platform::TestPlatformHooks;

static SCENARIO_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Eager test harness for `MonitorService`. Every builder method runs
/// immediately and panics on failure, so the offending line is the line that
/// fails. Public fields (`service`, `platform`, `api`, `clock`, `state_dir`)
/// are exposed so tests can reach past the DSL when the helpers don't cover
/// what they need.
pub struct Scenario {
    pub service: MonitorService<TestPlatformHooks, MockApiClient>,
    pub platform: TestPlatformHooks,
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
            let storage = FileStateStore::new(&state_dir).expect("create file state store");
            storage.save_auth_state(auth).expect("seed auth state");
        }
        let config = scenario_config(state_dir.clone());
        let clock = MockClock::default();
        let platform = TestPlatformHooks::with_clock(clock.clone());
        let api = MockApiClient::new();
        let api_handle = api.clone();
        let platform_handle = platform.clone();
        let mut service = MonitorService::setup_with_api(config, platform, api)
            .expect("scenario service must construct");
        if let Some(settings) = settings {
            service.upload_obs_mut().set_settings(Some(settings));
        }
        Self {
            service,
            platform: platform_handle,
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

    pub fn loop_iteration(&mut self) -> &mut Self {
        self.service
            .loop_iteration()
            .expect("loop_iteration must not error in scenarios; use try_loop_iteration for expected failures");
        self
    }

    pub fn try_loop_iteration(&mut self) -> CoreResult<()> {
        self.service.loop_iteration().map(|_| ())
    }

    pub fn queue_event(&mut self, event: Event) -> &mut Self {
        self.service.queue_event(event);
        self
    }

    pub fn shutdown(&mut self) -> &mut Self {
        self.service
            .queue_event(Event::ProcessStopped(ProcessStoppedReason::Shutdown));
        let _ = self.service.run_event_loop_iter();
        let _ = self.service.mark_stopped();
        self
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

    /// Pull the current `ServiceStatus` (reads from disk) and pass it to
    /// the closure. Closure should use `assert!` / `assert_eq!`.
    pub fn assert_status(&self, check: impl FnOnce(&ServiceStatus)) -> &Self {
        let status = self
            .service
            .status()
            .expect("fetch service status for assertion");
        check(&status);
        self
    }

    pub fn assert_is_running(&self, expected: bool) -> &Self {
        self.assert_status(|s| {
            assert_eq!(
                s.is_running, expected,
                "expected status.is_running = {expected}, got {}",
                s.is_running
            );
        })
    }

    pub fn assert_is_authenticated(&self, expected: bool) -> &Self {
        self.assert_status(|s| {
            assert_eq!(
                s.is_authenticated, expected,
                "expected status.is_authenticated = {expected}, got {}",
                s.is_authenticated
            );
        })
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

    /// Assert how many times the platform's `take_screenshot()` was called.
    pub fn assert_screenshot_call_count(&self, expected: u64) -> &Self {
        let actual = self.platform.take_call_count();
        assert_eq!(
            actual, expected,
            "expected take_screenshot call count {expected}, got {actual}"
        );
        self
    }

    /// Assert the number of pending upload requests
    /// (hash events + immediate events + 1 if any batch events are pending).
    pub fn assert_pending_request_count(&self, expected: usize) -> &Self {
        let actual = self.service.upload_obs().state.pending_request_count();
        assert_eq!(
            actual, expected,
            "expected {expected} pending requests, got {actual}"
        );
        self
    }

    // --- state setters (for fine-grained timing control in tests) ---

    /// Override the last-batch-uploaded timestamp.
    pub fn set_last_batch_at_ms(&mut self, ms: Option<i64>) -> &mut Self {
        self.service.upload_obs_mut().state.last_batch_at_ms = ms;
        self
    }

    /// Override the last-screenshot timestamp so interval-based tests can
    /// suppress or force a screenshot on the next loop.
    pub fn set_last_screenshot_at_ms(&mut self, ms: Option<i64>) -> &mut Self {
        self.service.event_loop.observers[1] // matches SCREENSHOT_IDX in service.rs
            .as_any_mut()
            .downcast_mut::<ScreenshotObserver>()
            .expect("screenshot observer at index 1")
            .state
            .last_screenshot_at_ms = ms;
        self
    }

    /// Override the full lifecycle observer state for fine-grained alert testing.
    pub fn set_lifecycle_observer_state(&mut self, state: LifecycleObserverState) -> &mut Self {
        self.service.event_loop.observers[0] // LIFECYCLE_IDX = 0
            .as_any_mut()
            .downcast_mut::<LifecycleObserver>()
            .expect("lifecycle observer at index 0")
            .state = state;
        self
    }

    // --- alternate constructors ---

    /// Build an authenticated service that reuses an *existing* state directory.
    /// Use this in restart/persistence tests where you want the second service
    /// to load state written by a first `Scenario`.
    ///
    /// The `Drop` impl will still attempt to remove the directory, but since
    /// both the first and second `Scenario` reference the same path the first
    /// successful removal is enough; the second attempt fails silently.
    pub fn authenticated_with_state_dir(state_dir: PathBuf) -> Self {
        let config = scenario_config(state_dir.clone());
        let clock = MockClock::default();
        let platform = TestPlatformHooks::with_clock(clock.clone());
        let api = MockApiClient::new();
        let api_handle = api.clone();
        let platform_handle = platform.clone();
        let service = MonitorService::setup_with_api(config, platform, api)
            .expect("scenario service must construct");
        Self {
            service,
            platform: platform_handle,
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
        // Best-effort cleanup. Ignored on failure so a panicking test still
        // reports its real cause.
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
