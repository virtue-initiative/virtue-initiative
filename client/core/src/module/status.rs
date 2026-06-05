use std::any::Any;
use std::sync::mpsc::Sender;

use crate::error::CoreResult;
use crate::events::{Event, Observer, StateType};
use crate::model::ServiceStatus;
use crate::platform::PlatformHooks;

pub struct StatusObserver {
    pub status: ServiceStatus,
    platform: Box<dyn PlatformHooks>,
    tx: Sender<Event>,
}

impl StatusObserver {
    pub fn new(
        initial_status: ServiceStatus,
        platform: Box<dyn PlatformHooks>,
        tx: Sender<Event>,
    ) -> Self {
        Self {
            status: initial_status,
            platform,
            tx,
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
            Event::Ping => {
                let now_ms = self.platform.get_time_utc_ms()?;
                self.status.last_loop_at_ms = Some(now_ms);
            }
            Event::Login { credentials, .. } => {
                self.status.is_authenticated = true;
                self.status.device_id = Some(credentials.device_id.clone());
            }
            Event::Logout => {
                self.status.is_authenticated = false;
                self.status.device_id = None;
            }
            Event::StatusRequest => {
                self.tx
                    .send(Event::StatusResponse {
                        status: self.status.clone(),
                    })
                    .ok();
            }
            _ => {}
        }
        Ok(())
    }
}
