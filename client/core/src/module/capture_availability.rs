use std::any::Any;
use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::{Event, Observer, StateType};
use crate::model::EventData;
use crate::platform::PlatformHooks;

const FAILURE_WINDOW_MS: i64 = 30 * 60 * 1_000;
const FAILURE_THRESHOLD: usize = 5;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct CaptureAvailabilityObserverState {
    pub recent_failures_ms: Vec<i64>,
}

pub struct CaptureAvailabilityObserver {
    pub state: CaptureAvailabilityObserverState,
    sender: Sender<Event>,
    platform: Box<dyn PlatformHooks>,
}

impl CaptureAvailabilityObserver {
    pub fn new(sender: Sender<Event>, platform: Box<dyn PlatformHooks>) -> Self {
        Self {
            state: CaptureAvailabilityObserverState::default(),
            sender,
            platform,
        }
    }
}

impl Observer for CaptureAvailabilityObserver {
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

    fn on_event(&mut self, event: &Event) -> CoreResult<()> {
        match event {
            Event::CaptureFailed => {
                let now_ms = self.platform.get_time_utc_ms()?;
                self.state.recent_failures_ms.push(now_ms);
                self.state
                    .recent_failures_ms
                    .retain(|&t| now_ms - t <= FAILURE_WINDOW_MS);
                if self.state.recent_failures_ms.len() >= FAILURE_THRESHOLD {
                    self.sender
                        .send(Event::Upload {
                            risk: 0.5,
                            kind: "capture_failure_spike".to_string(),
                            data: EventData::default(),
                        })
                        .ok();
                    self.state.recent_failures_ms.clear();
                }
            }
            _ => {}
        }
        Ok(())
    }
}
