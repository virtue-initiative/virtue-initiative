use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::error::CoreResult;
use crate::model::Screenshot;
use crate::platform::PlatformHooks;
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

    fn lock(&self) -> std::sync::MutexGuard<'_, TestPlatformInner> {
        self.inner.lock().expect("TestPlatformHooks state poisoned")
    }
}

impl Default for TestPlatformHooks {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformHooks for TestPlatformHooks {
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
}
