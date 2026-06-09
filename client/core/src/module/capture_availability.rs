use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::bus::{EventBus, Observer, StateType};
use crate::events::types::{CaptureFailed, Upload};
use crate::model::UploadKind;
use crate::platform::ScreenshotHooks;

const FAILURE_WINDOW_MS: i64 = 30 * 60 * 1_000;
const FAILURE_THRESHOLD: usize = 5;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct CaptureAvailabilityObserverState {
    pub recent_failures_ms: Vec<i64>,
}

pub(crate) struct CaptureAvailabilityInner {
    pub(crate) state: CaptureAvailabilityObserverState,
    platform: Box<dyn ScreenshotHooks>,
}

pub struct CaptureAvailabilityModule {
    pub(crate) inner: Arc<Mutex<CaptureAvailabilityInner>>,
}

impl CaptureAvailabilityModule {
    pub fn new(platform: Box<dyn ScreenshotHooks>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CaptureAvailabilityInner {
                state: CaptureAvailabilityObserverState::default(),
                platform,
            })),
        }
    }
}

impl Observer for CaptureAvailabilityModule {
    fn name(&self) -> &'static str {
        "capture_availability"
    }

    fn init(&mut self, bus: &mut EventBus, state: StateType) -> CoreResult<()> {
        if !state.is_null() {
            self.inner.lock().unwrap().state = serde_json::from_value(state)?;
        }

        let emitter = bus.emitter();

        let inner = Arc::clone(&self.inner);
        bus.subscribe(move |_: &CaptureFailed| {
            let mut g = inner.lock().unwrap();
            let now_ms = g.platform.get_time_utc_ms()?;
            g.state.recent_failures_ms.push(now_ms);
            g.state
                .recent_failures_ms
                .retain(|&t| now_ms - t <= FAILURE_WINDOW_MS);
            if g.state.recent_failures_ms.len() >= FAILURE_THRESHOLD {
                let _ = emitter.send(Upload {
                    risk: 0.5,
                    kind: UploadKind::CaptureFailed,
                });
                g.state.recent_failures_ms.clear();
            }
            Ok(())
        });

        Ok(())
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.inner.lock().unwrap().state)?)
    }
}
