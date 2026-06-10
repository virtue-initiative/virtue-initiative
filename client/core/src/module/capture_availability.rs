use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};
use crate::model::UploadKind;
use crate::module::screenshot::CaptureFailed;
use crate::module::upload::Upload;
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

#[cfg(test)]
mod tests {
    use super::CaptureAvailabilityModule;
    use crate::model::UploadKind;
    use crate::module::screenshot::CaptureFailed;
    use crate::module::upload::Upload;
    use crate::testing::EventTester;

    #[test]
    fn four_failures_below_threshold_no_upload() {
        let mut b = EventTester::builder();
        b.add(CaptureAvailabilityModule::new(Box::new(b.platform())));
        let mut t = b.build();
        for _ in 0..4 {
            t.emit(1, CaptureFailed);
        }
        assert!(
            t.captured::<Upload>().is_empty(),
            "4 failures should not trigger an upload"
        );
    }

    #[test]
    fn fifth_failure_triggers_capture_failed_upload() {
        let mut b = EventTester::builder();
        b.add(CaptureAvailabilityModule::new(Box::new(b.platform())));
        let mut t = b.build();
        for _ in 0..5 {
            t.emit(1, CaptureFailed);
        }
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::CaptureFailed,
            ..
        }));
    }
}
