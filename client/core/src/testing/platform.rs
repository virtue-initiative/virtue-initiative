use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::error::CoreResult;
use crate::model::Screenshot;
use crate::platform::{LifecycleHooks, ScreenshotHooks};
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
    last_login_utc_ms: Option<i64>,
    last_logout_utc_ms: Option<i64>,
    lifecycle_enabled: bool,
    /// When `Some`, overrides `get_monotonic_clock_ms()` instead of it
    /// mirroring `clock` (the default — see `set_monotonic_clock_override`).
    monotonic_override_ms: Option<i64>,
    /// `get_boot_id()`. The same for every instance by default, so a
    /// restarted scenario is on the same boot unless a test says otherwise.
    boot_id: Option<String>,
}

/// The boot ID every [`TestPlatformHooks`] reports until a test changes it.
const TEST_BOOT_ID: &str = "test-boot";

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
                last_login_utc_ms: None,
                last_logout_utc_ms: None,
                lifecycle_enabled: true,
                monotonic_override_ms: None,
                boot_id: Some(TEST_BOOT_ID.to_owned()),
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

    pub fn set_last_login(&self, utc_ms: Option<i64>) {
        self.lock().last_login_utc_ms = utc_ms;
    }

    pub fn set_last_logout(&self, utc_ms: Option<i64>) {
        self.lock().last_logout_utc_ms = utc_ms;
    }

    /// Mirrors `IosPlatformHooks::lifecycle_enabled() -> false` for tests
    /// that need to exercise the "lifecycle check disabled" path.
    pub fn set_lifecycle_enabled(&self, enabled: bool) {
        self.lock().lifecycle_enabled = enabled;
    }

    /// Diverges `get_monotonic_clock_ms()` from `clock` (the UTC clock) — by
    /// default it mirrors `clock` exactly, like every platform's real
    /// suspend-safe clock does while the system isn't suspended. Simulate a
    /// suspend by setting this to a fixed value and then advancing `clock`
    /// (real time passes, the suspend-safe clock doesn't); pass `None` to
    /// resume mirroring `clock` again (simulating a resume).
    pub fn set_monotonic_clock_override(&self, ms: Option<i64>) {
        self.lock().monotonic_override_ms = ms;
    }

    /// Sets what `get_boot_id()` returns: another ID simulates a reboot,
    /// `None` a platform that can't read one. Read once, when the daemon
    /// starts.
    pub fn set_boot_id(&self, boot_id: Option<&str>) {
        self.lock().boot_id = boot_id.map(str::to_owned);
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

    fn get_monotonic_clock_ms(&self) -> CoreResult<i64> {
        Ok(self
            .lock()
            .monotonic_override_ms
            .unwrap_or(self.clock.now_ms()))
    }

    fn get_boot_id(&self) -> CoreResult<Option<String>> {
        Ok(self.lock().boot_id.clone())
    }

    fn get_last_login_utc_ms(&self) -> CoreResult<Option<i64>> {
        Ok(self.lock().last_login_utc_ms)
    }

    fn get_last_logout_utc_ms(&self) -> CoreResult<Option<i64>> {
        Ok(self.lock().last_logout_utc_ms)
    }

    fn lifecycle_enabled(&self) -> bool {
        self.lock().lifecycle_enabled
    }
}
