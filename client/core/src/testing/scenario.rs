use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::config::Config;
use crate::daemon::{Daemon, DaemonState};
use crate::error::CoreResult;
use crate::model::{AuthState, BatchRecipient, DeviceCredentials, DeviceSettings, ServiceStatus};
use crate::module::upload::Upload;
use crate::rng::RandomSource;
use crate::testing::api::MockApiClient;
use crate::testing::clock::MockClock;
use crate::testing::platform::TestPlatformHooks;
use crate::testing::rng::TestRandomSource;

static SCENARIO_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Test harness wrapping a [`Daemon`] built via the same [`Daemon::new`]
/// production uses, so scenario tests exercise the real construction and
/// startup-refresh path rather than a hand-wired subset.
pub struct Scenario {
    pub daemon: Daemon<TestPlatformHooks, MockApiClient>,
    pub api: MockApiClient,
    pub clock: MockClock,
    pub state_dir: PathBuf,
    pub platform: TestPlatformHooks,
    pub rng: TestRandomSource,
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
            device_credentials: Some(DeviceCredentials {
                device_id: "scenario-device".into(),
                refresh_token: "scenario-device-refresh".into(),
            }),
        };
        let settings = DeviceSettings {
            device_id: "scenario-device".into(),
            name: "scenario device".into(),
            platform: "test-platform".into(),
            wrapping_keys: vec![BatchRecipient {
                user_id: "scenario-user".into(),
                // X25519 base point (u=9); any valid curve point works here.
                pub_key_base64: "CQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            }],
            hash_base_url: None,
        };
        Self::build(Some(auth), Some(settings))
    }

    fn build(auth: Option<AuthState>, settings: Option<DeviceSettings>) -> Self {
        let state_dir = scenario_temp_dir();

        if let Some(ref auth) = auth {
            let event_state = serde_json::json!({
                "auth": auth,
                "screenshot": { "enabled": true, "next_screenshot_at_ms": null },
                "upload": { "device_credentials": auth.device_credentials },
            });
            let path = state_dir.join("event_state.json");
            fs::write(&path, serde_json::to_vec_pretty(&event_state).unwrap())
                .expect("seed event state with auth");
        }

        Self::from_state_dir(state_dir, settings)
    }

    fn from_state_dir(state_dir: PathBuf, settings: Option<DeviceSettings>) -> Self {
        let config = scenario_config(state_dir.clone());
        let clock = MockClock::default();
        let platform = TestPlatformHooks::with_clock(clock.clone());
        let api = MockApiClient::new();
        if let Some(ref s) = settings {
            api.state().default_device_settings = s.clone();
        }
        let api_handle = api.clone();
        let rng = TestRandomSource::new();

        let state_path = state_dir.join("event_state.json");
        let daemon = Daemon::new(config, platform.clone(), api, state_path)
            .expect("scenario daemon must construct")
            .with_rng(Arc::new(rng.clone()) as Arc<dyn RandomSource>);

        Self {
            daemon,
            api: api_handle,
            clock,
            state_dir,
            platform,
            rng,
        }
    }

    /// Build an authenticated service that reuses an *existing* state
    /// directory — used to test state surviving a daemon restart.
    pub fn authenticated_with_state_dir(state_dir: PathBuf) -> Self {
        Self::from_state_dir(state_dir, None)
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

    // --- driving the daemon ---

    /// Run one tick at the current clock time, single-threaded (no
    /// `run_forever` loop thread involved).
    pub fn tick(&mut self) -> &mut Self {
        self.daemon.tick_once_for_test(self.clock.now_ms());
        self
    }

    pub fn tick_n(&mut self, n: usize) -> &mut Self {
        for _ in 0..n {
            self.tick();
        }
        self
    }

    pub fn login(
        &mut self,
        email: &str,
        password: &str,
        device_name: Option<&str>,
    ) -> CoreResult<String> {
        self.daemon.test_login(email, password, device_name)
    }

    pub fn logout(&mut self) -> CoreResult<()> {
        self.daemon.test_logout()
    }

    pub fn status(&mut self) -> ServiceStatus {
        self.daemon.status()
    }

    pub fn note_user_stop(&mut self, source: &str) -> &mut Self {
        self.daemon.test_note_user_stop(source);
        self
    }

    pub fn note_user_start(&mut self) -> &mut Self {
        self.daemon.test_note_user_start();
        self
    }

    pub fn queue_upload(&mut self, upload: Upload) -> &mut Self {
        self.daemon.test_queue_upload(upload);
        self
    }

    pub fn flush_batch_now(&mut self) -> &mut Self {
        self.daemon.test_flush_batch_now();
        self
    }

    pub fn force_capture_now(&mut self) -> &mut Self {
        self.daemon.test_force_capture();
        self
    }

    /// A cloned snapshot of the daemon's current state.
    pub fn state(&self) -> DaemonState {
        self.daemon.state_snapshot()
    }

    /// Run `f` with exclusive access to the daemon's live state, e.g. to
    /// seed a scenario precondition.
    pub fn with_state_mut<R>(&mut self, f: impl FnOnce(&mut DaemonState) -> R) -> R {
        self.daemon.with_state_mut(f)
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

    // --- assertions ---

    pub fn assert_batch_upload_count(&self, expected: usize) -> &Self {
        let actual = self.api.state().batch_uploads.len();
        assert_eq!(
            actual, expected,
            "expected {expected} batch uploads, recorded {actual}"
        );
        self
    }

    pub fn assert_notify_count(&self, expected: usize) -> &Self {
        let actual = self.api.state().notify_calls.len();
        assert_eq!(
            actual, expected,
            "expected {expected} notify calls, recorded {actual}"
        );
        self
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
        Duration::from_secs(60),
        Duration::from_secs(60),
    )
}
