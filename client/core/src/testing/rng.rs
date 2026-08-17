use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::rng::RandomSource;

/// Test [`RandomSource`]: serves a queue of canned draws, falling back to a
/// fixed default (`0.5`) once the queue is empty — mirrors
/// [`TestPlatformHooks`](super::TestPlatformHooks)'s queued-screenshot pattern.
#[derive(Clone)]
pub struct TestRandomSource {
    queued: Arc<Mutex<VecDeque<f64>>>,
    default: Arc<Mutex<f64>>,
}

impl TestRandomSource {
    pub fn new() -> Self {
        Self {
            queued: Arc::new(Mutex::new(VecDeque::new())),
            default: Arc::new(Mutex::new(0.5)),
        }
    }

    /// Queue a canned draw, consumed in FIFO order on the next `uniform()` call.
    pub fn queue(&self, u: f64) {
        self.queued.lock().unwrap().push_back(u);
    }

    /// Change the fallback value served once the queue is empty.
    pub fn set_default(&self, u: f64) {
        *self.default.lock().unwrap() = u;
    }
}

impl Default for TestRandomSource {
    fn default() -> Self {
        Self::new()
    }
}

impl RandomSource for TestRandomSource {
    fn uniform(&self) -> f64 {
        self.queued
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| *self.default.lock().unwrap())
    }
}
