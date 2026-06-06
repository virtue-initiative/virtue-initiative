use std::any::Any;

use crate::error::CoreResult;
use crate::events::{Event, Observer, StateType};
use crate::ipc::IpcSender;

pub struct RequestObserver {
    clients: Vec<IpcSender>,
}

impl Default for RequestObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestObserver {
    pub fn new() -> Self {
        Self {
            clients: Vec::new(),
        }
    }

    pub fn add_client(&mut self, sender: IpcSender) {
        self.clients.push(sender);
    }
}

impl Observer for RequestObserver {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "request_handler"
    }

    fn save_state(&self) -> CoreResult<StateType> {
        Ok(serde_json::Value::Null)
    }

    fn load_state(&mut self, _state: StateType) -> CoreResult<()> {
        Ok(())
    }

    fn on_event(&mut self, event: &Event) -> CoreResult<()> {
        // Forward events to connected controllers; skip large data events,
        // internal auth events that carry sensitive credentials, and the
        // intra-loop status fragments that assemble a StatusResponse.
        if !matches!(
            event,
            Event::Upload { .. } | Event::Ping | Event::Login { .. } | Event::PartialStatus(_)
        ) {
            self.clients.retain_mut(|c| c.send(event).is_ok());
        }
        Ok(())
    }
}
