use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::error::CoreResult;
use crate::model::Screenshot;
use crate::platform::{LifecycleHooks, PlatformHooks, ScreenshotHooks};
use crate::testing::clock::MockClock;
use crate::testing::fixtures::tiny_png_screenshot;

#[derive(Clone)]
pub struct TestPlatformHooks {
    pub clock: MockClock,
    inner: Arc<Mutex<TestPlatformInner>>,
}

struct TestPlatformInner {
    queued_screenshots: VecDeque<CoreResult<Screenshot>>,
    take_call_count: u64,
    default_screenshot: Screenshot,
    locked_or_screensaver: bool,
    boot_clock_override_ms: Option<i64>,
    monotonic_clock_override_ms: Option<i64>,
    last_login_utc_ms: Option<i64>,
    last_logout_utc_ms: Option<i64>,
}

impl TestPlatformHooks {
    pub fn new() -> Self {
        Self::with_clock(MockClock::default())
    }

    pub fn with_clock(clock: MockClock) -> Self {
        Self {
            clock,
            inner: Arc::new(Mutex::new(TestPlatformInner {
                queued_screenshots: VecDeque::new(),
                take_call_count: 0,
                default_screenshot: tiny_png_screenshot(),
                locked_or_screensaver: false,
                boot_clock_override_ms: None,
                monotonic_clock_override_ms: None,
                last_login_utc_ms: None,
                last_logout_utc_ms: None,
            })),
        }
    }

    pub fn queue_screenshot(&self, result: CoreResult<Screenshot>) {
        self.lock().queued_screenshots.push_back(result);
    }

    pub fn set_default_screenshot(&self, screenshot: Screenshot) {
        self.lock().default_screenshot = screenshot;
    }

    pub fn take_call_count(&self) -> u64 {
        self.lock().take_call_count
    }

    pub fn set_locked_or_screensaver(&self, locked: bool) {
        self.lock().locked_or_screensaver = locked;
    }

    /// Override the boot clock returned by `get_boot_clock_ms`. Sticky until
    /// overridden again; with no override, it tracks the shared `MockClock`
    /// in lockstep (normal operation, no suspend/reboot).
    pub fn set_boot_clock_ms(&self, ms: i64) {
        self.lock().boot_clock_override_ms = Some(ms);
    }

    /// Override the monotonic clock returned by `get_monotonic_clock_ms`.
    /// Sticky until overridden again. Freezing this while the wall clock (and
    /// an unfrozen boot clock) advance simulates a suspend; resetting both
    /// boot and monotonic to a small value while the wall clock keeps
    /// climbing simulates a reboot.
    pub fn set_monotonic_clock_ms(&self, ms: i64) {
        self.lock().monotonic_clock_override_ms = Some(ms);
    }

    pub fn set_last_login(&self, utc_ms: Option<i64>) {
        self.lock().last_login_utc_ms = utc_ms;
    }

    pub fn set_last_logout(&self, utc_ms: Option<i64>) {
        self.lock().last_logout_utc_ms = utc_ms;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, TestPlatformInner> {
        self.inner.lock().expect("TestPlatformHooks state poisoned")
    }
}

impl Default for TestPlatformHooks {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenshotHooks for TestPlatformHooks {
    fn take_screenshot(&self) -> CoreResult<Screenshot> {
        let mut inner = self.lock();
        inner.take_call_count += 1;
        if let Some(queued) = inner.queued_screenshots.pop_front() {
            queued
        } else {
            Ok(inner.default_screenshot.clone())
        }
    }

    fn get_time_utc_ms(&self) -> CoreResult<i64> {
        Ok(self.clock.now_ms())
    }

    fn is_locked_or_screensaver(&self) -> CoreResult<bool> {
        Ok(self.lock().locked_or_screensaver)
    }
}

impl LifecycleHooks for TestPlatformHooks {
    fn get_utc_clock_ms(&self) -> CoreResult<i64> {
        Ok(self.clock.now_ms())
    }

    fn get_boot_clock_ms(&self) -> CoreResult<i64> {
        Ok(self
            .lock()
            .boot_clock_override_ms
            .unwrap_or_else(|| self.clock.now_ms()))
    }

    fn get_monotonic_clock_ms(&self) -> CoreResult<i64> {
        Ok(self
            .lock()
            .monotonic_clock_override_ms
            .unwrap_or_else(|| self.clock.now_ms()))
    }

    fn get_last_login_utc_ms(&self) -> CoreResult<Option<i64>> {
        Ok(self.lock().last_login_utc_ms)
    }

    fn get_last_logout_utc_ms(&self) -> CoreResult<Option<i64>> {
        Ok(self.lock().last_logout_utc_ms)
    }
}

impl PlatformHooks for TestPlatformHooks {}
