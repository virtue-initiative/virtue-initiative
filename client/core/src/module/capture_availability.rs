use std::any::Any;
use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::{Event, Observer, StateType, UploadKind};
use crate::platform::ScreenshotHooks;

const FAILURE_WINDOW_MS: i64 = 30 * 60 * 1_000;
const FAILURE_THRESHOLD: usize = 5;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct CaptureAvailabilityObserverState {
    pub recent_failures_ms: Vec<i64>,
}

pub struct CaptureAvailabilityObserver<C = ()> {
    pub state: CaptureAvailabilityObserverState,
    sender: Sender<Event<C>>,
    platform: Box<dyn ScreenshotHooks>,
}

impl<C: 'static> CaptureAvailabilityObserver<C> {
    pub fn new(sender: Sender<Event<C>>, platform: Box<dyn ScreenshotHooks>) -> Self {
        Self {
            state: CaptureAvailabilityObserverState::default(),
            sender,
            platform,
        }
    }
}

impl<C: 'static> Observer<C> for CaptureAvailabilityObserver<C> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "capture_availability"
    }

    fn save_state(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.state)?)
    }

    fn load_state(&mut self, state: StateType) -> CoreResult<()> {
        self.state = serde_json::from_value(state)?;
        Ok(())
    }

    fn on_event(&mut self, event: &Event<C>) -> CoreResult<()> {
        if let Event::CaptureFailed = event {
            let now_ms = self.platform.get_time_utc_ms()?;
            self.state.recent_failures_ms.push(now_ms);
            self.state
                .recent_failures_ms
                .retain(|&t| now_ms - t <= FAILURE_WINDOW_MS);
            if self.state.recent_failures_ms.len() >= FAILURE_THRESHOLD {
                self.sender
                    .send(Event::Upload {
                        risk: 0.5,
                        kind: UploadKind::CaptureFailed,
                    })
                    .ok();
                self.state.recent_failures_ms.clear();
            }
        }
        Ok(())
    }
}
