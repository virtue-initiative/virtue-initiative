use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

#[derive(Clone, Debug)]
pub struct MockClock {
    now_ms: Arc<AtomicI64>,
}

impl MockClock {
    pub fn new(initial_ms: i64) -> Self {
        Self {
            now_ms: Arc::new(AtomicI64::new(initial_ms)),
        }
    }

    pub fn now_ms(&self) -> i64 {
        self.now_ms.load(Ordering::Relaxed)
    }

    pub fn set(&self, ms: i64) {
        self.now_ms.store(ms, Ordering::Relaxed);
    }

    pub fn advance(&self, delta_ms: i64) {
        self.now_ms.fetch_add(delta_ms, Ordering::Relaxed);
    }
}

impl Default for MockClock {
    fn default() -> Self {
        Self::new(0)
    }
}
