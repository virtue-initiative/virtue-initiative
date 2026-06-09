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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::StatusModule;
    use crate::events::bus::{EventBus, StateType};
    use crate::events::types::{PartialStatus, StatusRequest, StatusResponse};

    fn make(expected_count: usize) -> (EventBus, Arc<Mutex<Vec<StatusResponse>>>) {
        let module = StatusModule::new(expected_count);
        let responses: Arc<Mutex<Vec<StatusResponse>>> = Arc::new(Mutex::new(Vec::new()));
        let r = Arc::clone(&responses);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &StatusResponse| {
            r.lock().unwrap().push(ev.clone());
            Ok(())
        });
        (bus, responses)
    }

    #[test]
    fn one_partial_with_expected_one_triggers_response() {
        let (mut bus, responses) = make(1);
        bus.send(StatusRequest).unwrap();
        bus.send(PartialStatus::Auth {
            is_authenticated: true,
            device_id: Some("dev".into()),
        })
        .unwrap();
        bus.iter().unwrap();
        assert_eq!(
            responses.lock().unwrap().len(),
            1,
            "expected StatusResponse"
        );
    }

    #[test]
    fn response_only_after_all_expected_fragments_received() {
        let (mut bus, responses) = make(3);
        bus.send(StatusRequest).unwrap();
        bus.send(PartialStatus::Auth {
            is_authenticated: false,
            device_id: None,
        })
        .unwrap();
        bus.iter().unwrap();
        assert!(
            responses.lock().unwrap().is_empty(),
            "should not respond after 1 of 3"
        );

        bus.send(PartialStatus::Lifecycle {
            is_running: true,
            last_loop_at_ms: Some(1_000),
        })
        .unwrap();
        bus.iter().unwrap();
        assert!(
            responses.lock().unwrap().is_empty(),
            "should not respond after 2 of 3"
        );

        bus.send(PartialStatus::Upload {
            pending_request_count: 7,
        })
        .unwrap();
        bus.iter().unwrap();
        let r = responses.lock().unwrap();
        assert_eq!(r.len(), 1);
        assert!(!r[0].status.is_authenticated);
        assert!(r[0].status.is_running);
        assert_eq!(r[0].status.last_loop_at_ms, Some(1_000));
        assert_eq!(r[0].status.pending_request_count, 7);
    }

    #[test]
    fn new_status_request_resets_accumulated_state() {
        let (mut bus, responses) = make(1);
        bus.send(StatusRequest).unwrap();
        bus.send(PartialStatus::Auth {
            is_authenticated: true,
            device_id: Some("dev1".into()),
        })
        .unwrap();
        bus.iter().unwrap();
        assert_eq!(responses.lock().unwrap().len(), 1);
        responses.lock().unwrap().clear();

        bus.send(StatusRequest).unwrap();
        bus.send(PartialStatus::Auth {
            is_authenticated: false,
            device_id: None,
        })
        .unwrap();
        bus.iter().unwrap();
        let r = responses.lock().unwrap();
        assert_eq!(r.len(), 1);
        assert!(!r[0].status.is_authenticated);
    }
}
