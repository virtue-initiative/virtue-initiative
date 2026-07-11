pub mod api;
pub mod clock;
pub mod event_tester;
pub mod fixtures;
pub mod platform;
pub mod scenario;
pub mod spawner;

pub use api::{BatchCall, HashCall, MockApiClient, MockApiState, NotifyCall, RegisterDeviceCall};
pub use clock::MockClock;
pub use event_tester::{EventTester, EventTesterBuilder};
pub use fixtures::{tiny_png_bytes, tiny_png_screenshot};
pub use platform::TestPlatformHooks;
pub use scenario::Scenario;
pub use spawner::InlineSpawner;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use super::*;
    use crate::api::ApiTransport;
    use crate::assembly::build_default_modules;
    use crate::config::Config;
    use crate::events::Ping;
    use crate::events::bus::{EventBus, EventChannel, StateType};
    use crate::module::status::{StatusRequest, StatusResponse};
    use crate::platform::{PlatformConfig, ScreenshotHooks};

    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_state_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "virtue-core-testing-{}-{}",
            std::process::id(),
            TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temp state dir");
        path
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

    #[test]
    fn mock_clock_advances_and_sets() {
        let clock = MockClock::new(1_000);
        assert_eq!(clock.now_ms(), 1_000);
        clock.advance(500);
        assert_eq!(clock.now_ms(), 1_500);
        clock.set(42);
        assert_eq!(clock.now_ms(), 42);
    }

    #[test]
    fn test_platform_serves_queued_then_default_screenshots() {
        let platform = TestPlatformHooks::new();
        platform.clock.set(123);
        assert_eq!(platform.get_time_utc_ms().unwrap(), 123);

        let first = platform.take_screenshot().expect("default screenshot");
        assert_eq!(first.content_type, "image/png");
        assert_eq!(platform.take_call_count(), 1);

        let mut custom = tiny_png_screenshot();
        custom.captured_at_ms = 999;
        platform.queue_screenshot(Ok(custom));

        let second = platform.take_screenshot().expect("queued screenshot");
        assert_eq!(second.captured_at_ms, 999);
        assert_eq!(platform.take_call_count(), 2);
    }

    #[test]
    fn mock_api_client_records_calls_and_serves_canned_responses() {
        let api = MockApiClient::new();
        let inspector = api.clone();

        api.login("alice@example.org", "secret").unwrap();
        assert_eq!(inspector.state().login_calls.len(), 1);

        api.program_batch(Ok(crate::api::UploadedBatchResponse {
            id: "canned-batch".into(),
        }));
        let batch = crate::model::BatchUpload {
            start_time_ms: 0,
            end_time_ms: 1,
            bytes: vec![1, 2, 3],
            access_keys: Vec::new(),
            high_risk_count: 0,
            medium_risk_count: 0,
        };
        let response = api.upload_batch("tok", &batch).unwrap();
        assert_eq!(response.id, "canned-batch");
        assert_eq!(inspector.state().batch_uploads.len(), 1);

        let second = api.upload_batch("tok", &batch).unwrap();
        assert_eq!(second.id, "mock-batch-1");
    }

    #[test]
    fn bus_with_default_modules_constructs_and_runs_one_ping() {
        let state_dir = temp_state_dir();
        let config = test_config(state_dir.clone());
        let platform = TestPlatformHooks::new();
        let api = MockApiClient::new();
        let inspector = api.clone();

        let observers = build_default_modules(config, platform, api, PlatformConfig::default())
            .expect("build modules must succeed");
        let mut bus = EventBus::new(observers, StateType::Null).expect("bus must construct");

        // No auth state → ping must not upload anything.
        bus.send(Ping).unwrap();
        bus.iter().unwrap();

        let state = inspector.state();
        assert!(
            state.batch_uploads.is_empty(),
            "unauthenticated loop must not upload batches"
        );
        assert!(
            state.notify_calls.is_empty(),
            "unauthenticated loop must not send notifications"
        );

        // Status request must return a valid response.
        let status: StatusResponse = bus.request(StatusRequest).unwrap();
        assert!(!status.status.is_authenticated);
        assert!(status.status.is_running);

        fs::remove_dir_all(&state_dir).ok();
    }
}
