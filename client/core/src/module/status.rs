use std::sync::{Arc, Mutex};

use crate::error::CoreResult;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};
use crate::events::types::{PartialStatus, StatusRequest, StatusResponse};
use crate::model::ServiceStatus;

struct StatusInner {
    expected_count: usize,
    received: usize,
    pending: ServiceStatus,
}

impl StatusInner {
    fn merge(&mut self, partial: &PartialStatus) {
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
    }

    fn handle_partial(&mut self, partial: &PartialStatus, emitter: &Emitter) {
        self.merge(partial);
        self.received += 1;
        if self.received >= self.expected_count {
            let _ = emitter.send(StatusResponse {
                status: self.pending.clone(),
            });
            self.received = 0;
        }
    }
}

pub struct StatusModule {
    inner: Arc<Mutex<StatusInner>>,
}

impl StatusModule {
    pub fn new(expected_count: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(StatusInner {
                expected_count,
                received: 0,
                pending: ServiceStatus::default(),
            })),
        }
    }
}

impl Observer for StatusModule {
    fn name(&self) -> &'static str {
        "status"
    }

    fn init(&mut self, bus: &mut EventBus, _state: StateType) -> CoreResult<()> {
        let emitter = bus.emitter();

        let inner = Arc::clone(&self.inner);
        bus.subscribe(move |_: &StatusRequest| {
            let mut g = inner.lock().unwrap();
            g.pending = ServiceStatus::default();
            g.received = 0;
            Ok(())
        });

        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |partial: &PartialStatus| {
            inner.lock().unwrap().handle_partial(partial, &e);
            Ok(())
        });

        Ok(())
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(StateType::Null)
    }
}
