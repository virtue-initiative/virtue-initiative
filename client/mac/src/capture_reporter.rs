use std::any::Any;
use std::sync::mpsc::Sender;

use virtue_core::CoreResult;
use virtue_core::events::{Event, Observer, StateType};

use crate::capture::{MacEvent, has_screen_capture_access};

/// Observer that monitors macOS screen-capture permission availability and
/// emits `Event::Custom(MacEvent::CaptureAvailabilityChanged)` when it changes.
pub struct CaptureReporterObserver {
    sender: Sender<Event<MacEvent>>,
    last_available: Option<bool>,
}

impl CaptureReporterObserver {
    pub fn new(sender: Sender<Event<MacEvent>>) -> Self {
        Self {
            sender,
            last_available: None,
        }
    }
}

impl Observer<MacEvent> for CaptureReporterObserver {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "capture_reporter"
    }

    fn save_state(&self) -> CoreResult<StateType> {
        Ok(serde_json::Value::Null)
    }

    fn load_state(&mut self, _state: StateType) -> CoreResult<()> {
        Ok(())
    }

    fn on_event(&mut self, event: &Event<MacEvent>) -> CoreResult<()> {
        match event {
            Event::Ping => {
                let available = has_screen_capture_access();
                if self.last_available != Some(available) {
                    self.last_available = Some(available);
                    self.sender
                        .send(Event::Custom(MacEvent::CaptureAvailabilityChanged(
                            available,
                        )))
                        .ok();
                }
            }
            Event::StatusRequest => {
                if let Some(available) = self.last_available {
                    self.sender
                        .send(Event::Custom(MacEvent::CaptureAvailabilityChanged(
                            available,
                        )))
                        .ok();
                }
            }
            _ => {}
        }
        Ok(())
    }
}
