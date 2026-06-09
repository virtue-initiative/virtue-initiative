use std::any::Any;

use crate::error::CoreResult;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};
use crate::events::types::{PartialStatus, StatusRequest, StatusResponse};
use crate::model::ServiceStatus;

pub struct StatusModule {
    expected_count: usize,
    received: usize,
    pending: ServiceStatus,
}

impl StatusModule {
    pub fn new(expected_count: usize) -> Self {
        Self {
            expected_count,
            received: 0,
            pending: ServiceStatus::default(),
        }
    }

    fn handle_partial(&mut self, partial: &PartialStatus, emitter: &Emitter) {
        match partial {
            PartialStatus::Auth {
                is_authenticated,
                device_id,
            } => {
                self.pending.is_authenticated = *is_authenticated;
                self.pending.device_id = device_id.clone();
            }
            PartialStatus::Lifecycle {
                is_running,
                last_loop_at_ms,
            } => {
                self.pending.is_running = *is_running;
                self.pending.last_loop_at_ms = *last_loop_at_ms;
            }
            PartialStatus::Upload {
                pending_request_count,
            } => {
                self.pending.pending_request_count = *pending_request_count;
            }
        }
        self.received += 1;
        if self.received >= self.expected_count {
            let _ = emitter.send(StatusResponse {
                status: self.pending.clone(),
            });
            self.received = 0;
        }
    }
}

impl Observer for StatusModule {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "status"
    }

    fn init(&mut self, _bus: &mut EventBus, _state: StateType) -> CoreResult<()> {
        Ok(())
    }

    fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
        crate::dispatch_event!(event, {
            _: StatusRequest => {
                self.pending = ServiceStatus::default();
                self.received = 0;
                Ok(())
            },
            partial: PartialStatus => {
                self.handle_partial(partial, emitter);
                Ok(())
            },
        })
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(StateType::Null)
    }
}
