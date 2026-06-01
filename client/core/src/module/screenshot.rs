pub mod image_pipeline;

use std::any::Any;
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::{Event, Observer, StateType};
use crate::model::EventData;
use crate::platform::PlatformHooks;

pub struct ScreenshotConfig {
    pub screenshot_interval: Duration,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct ScreenshotObserverState {
    pub last_screenshot_at_ms: Option<i64>,
}

pub struct ScreenshotObserver {
    pub state: ScreenshotObserverState,
    platform: Box<dyn PlatformHooks>,
    pub config: ScreenshotConfig,
    sender: Sender<Event>,
}

impl ScreenshotObserver {
    pub fn new(
        platform: Box<dyn PlatformHooks>,
        sender: Sender<Event>,
        config: ScreenshotConfig,
    ) -> Self {
        Self {
            state: ScreenshotObserverState::default(),
            platform,
            config,
            sender,
        }
    }

    pub fn reset_schedule(&mut self) {
        self.state.last_screenshot_at_ms = None;
    }
}

impl Observer for ScreenshotObserver {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "screenshot"
    }

    fn save_state(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.state)?)
    }

    fn load_state(&mut self, state: StateType) -> CoreResult<()> {
        self.state = serde_json::from_value(state)?;
        Ok(())
    }

    fn on_event(&mut self, event: &Event) -> CoreResult<()> {
        if !matches!(event, Event::Ping) {
            return Ok(());
        }
        let now_ms = self.platform.get_time_utc_ms()?;
        let interval_ms = self.config.screenshot_interval.as_millis() as i64;
        let should = self
            .state
            .last_screenshot_at_ms
            .map(|last| now_ms - last >= interval_ms)
            .unwrap_or(true);
        if !should {
            return Ok(());
        }
        let screenshot = match self.platform.take_screenshot() {
            Ok(s) => s,
            Err(e) => {
                self.sender.send(Event::CaptureFailed).ok();
                return Err(e);
            }
        };
        let processed = image_pipeline::ImagePipeline.process(screenshot)?;
        let data = EventData::default().with_screenshot(processed.bytes, processed.content_type);
        self.state.last_screenshot_at_ms = Some(now_ms);
        self.sender
            .send(Event::Upload {
                risk: 0.0,
                kind: "screenshot".to_string(),
                data,
            })
            .ok();
        Ok(())
    }
}
