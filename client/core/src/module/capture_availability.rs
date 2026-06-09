use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};
use crate::events::types::{CaptureFailed, Upload};
use crate::model::UploadKind;
use crate::platform::ScreenshotHooks;

const FAILURE_WINDOW_MS: i64 = 30 * 60 * 1_000;
const FAILURE_THRESHOLD: usize = 5;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct CaptureAvailabilityObserverState {
    pub recent_failures_ms: Vec<i64>,
}

pub struct CaptureAvailabilityModule {
    pub state: CaptureAvailabilityObserverState,
    platform: Box<dyn ScreenshotHooks>,
}

impl CaptureAvailabilityModule {
    pub fn new(platform: Box<dyn ScreenshotHooks>) -> Self {
        Self {
            state: CaptureAvailabilityObserverState::default(),
            platform,
        }
    }
}

impl Observer for CaptureAvailabilityModule {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "capture_availability"
    }

    fn init(&mut self, _bus: &mut EventBus, state: StateType) -> CoreResult<()> {
        if !state.is_null() {
            self.state = serde_json::from_value(state)?;
        }
        Ok(())
    }

    fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
        crate::dispatch_event!(event, {
            _: CaptureFailed => {
                let now_ms = self.platform.get_time_utc_ms()?;
                self.state.recent_failures_ms.push(now_ms);
                self.state.recent_failures_ms.retain(|&t| now_ms - t <= FAILURE_WINDOW_MS);
                if self.state.recent_failures_ms.len() >= FAILURE_THRESHOLD {
                    let _ = emitter.send(Upload {
                        risk: 0.5,
                        kind: UploadKind::CaptureFailed,
                    });
                    self.state.recent_failures_ms.clear();
                }
                Ok(())
            },
        })
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.state)?)
    }
}
