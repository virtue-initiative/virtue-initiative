use std::any::Any;

use virtue_core::{CoreResult, Emitter, EventBus, Observer, Ping, StateType, StatusRequest};

use crate::capture::{CaptureAvailabilityChanged, has_screen_capture_access};

/// Observer that monitors macOS screen-capture permission availability and
/// emits `CaptureAvailabilityChanged` when it changes or on `StatusRequest`.
pub struct CaptureReporterModule {
    last_available: Option<bool>,
}

impl CaptureReporterModule {
    pub fn new() -> Self {
        Self {
            last_available: None,
        }
    }
}

impl Observer for CaptureReporterModule {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "capture_reporter"
    }

    fn init(&mut self, _bus: &mut EventBus, _state: StateType) -> CoreResult<()> {
        Ok(())
    }

    fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
        virtue_core::dispatch_event!(event, {
            _: Ping => {
                let available = has_screen_capture_access();
                if self.last_available != Some(available) {
                    self.last_available = Some(available);
                    let _ = emitter.send(CaptureAvailabilityChanged(available));
                }
                Ok(())
            },
            _: StatusRequest => {
                if let Some(available) = self.last_available {
                    let _ = emitter.send(CaptureAvailabilityChanged(available));
                }
                Ok(())
            },
        })
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(StateType::Null)
    }
}
