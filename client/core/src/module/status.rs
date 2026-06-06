use std::any::Any;
use std::sync::mpsc::Sender;

use crate::error::CoreResult;
use crate::events::{Event, Observer, PartialStatus, StateType};
use crate::model::ServiceStatus;

/// Assembles a `StatusResponse` from the `PartialStatus` fragments that other
/// observers emit in reply to a `StatusRequest`. Holds no persistent state of
/// its own: it accumulates fragments transiently and, once it has heard from
/// every expected observer, sends the combined `ServiceStatus` and resets.
pub struct StatusObserver {
    /// Number of `PartialStatus` fragments to wait for before responding.
    expected_count: usize,
    /// Fragments received since the current `StatusRequest`.
    received: usize,
    /// Status assembled from fragments received so far.
    pending: ServiceStatus,
    tx: Sender<Event>,
}

impl StatusObserver {
    pub fn new(expected_count: usize, tx: Sender<Event>) -> Self {
        Self {
            expected_count,
            received: 0,
            pending: ServiceStatus::default(),
            tx,
        }
    }

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
}

impl Observer for StatusObserver {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "status"
    }

    fn save_state(&self) -> CoreResult<StateType> {
        Ok(serde_json::Value::Null)
    }

    fn load_state(&mut self, _state: StateType) -> CoreResult<()> {
        Ok(())
    }

    fn on_event(&mut self, event: &Event) -> CoreResult<()> {
        match event {
            Event::StatusRequest => {
                // Begin collecting fragments for a fresh response.
                self.pending = ServiceStatus::default();
                self.received = 0;
            }
            Event::PartialStatus(partial) => {
                self.merge(partial);
                self.received += 1;
                if self.received >= self.expected_count {
                    self.tx
                        .send(Event::StatusResponse {
                            status: self.pending.clone(),
                        })
                        .ok();
                    self.received = 0;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
