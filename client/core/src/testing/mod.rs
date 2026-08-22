pub mod api;
pub mod clock;
pub mod fixtures;
pub mod platform;
pub mod rng;
pub mod scenario;

pub use api::{BatchCall, HashCall, MockApiClient, MockApiState, NotifyCall, RegisterDeviceCall};
pub use clock::MockClock;
pub use fixtures::{tiny_png_bytes, tiny_png_screenshot};
pub use platform::TestPlatformHooks;
pub use rng::TestRandomSource;
pub use scenario::Scenario;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiTransport;
    use crate::platform::ScreenshotHooks;

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

        api.register_device(
            "alice@example.org",
            "secret",
            "test-device",
            "test-platform",
        )
        .unwrap();
        assert_eq!(inspector.state().register_device_calls.len(), 1);

        let default_settings = inspector.state().default_device_settings.clone();
        api.program_batch(Ok(crate::api::UploadedBatchResponse {
            id: "canned-batch".into(),
            settings: default_settings,
            hash_token: "canned-hash-token".into(),
        }));
        let batch = crate::model::BatchUpload {
            start_time_ms: 0,
            end_time_ms: 1,
            bytes: vec![1, 2, 3],
            access_keys: Vec::new(),
            total_count: 0,
            high_risk_count: 0,
            medium_risk_count: 0,
            screenshot_count: 0,
            notifications: Vec::new(),
        };
        let response = api.upload_batch("tok", &batch).unwrap();
        assert_eq!(response.id, "canned-batch");
        assert_eq!(inspector.state().batch_uploads.len(), 1);

        let second = api.upload_batch("tok", &batch).unwrap();
        assert_eq!(second.id, "mock-batch-1");
    }

    #[test]
    fn test_random_source_serves_queued_then_default() {
        let rng = TestRandomSource::new();
        rng.queue(0.1);
        rng.queue(0.2);
        assert_eq!(rng.uniform(), 0.1);
        assert_eq!(rng.uniform(), 0.2);
        assert_eq!(rng.uniform(), 0.5);
        use crate::rng::RandomSource;
        let _ = &rng as &dyn RandomSource;
    }
}
