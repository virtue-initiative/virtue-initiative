pub mod image_pipeline;

use std::any::Any;
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::{Event, Observer, StateType, UploadKind};
use crate::platform::ScreenshotHooks;

pub struct ScreenshotConfig {
    pub screenshot_interval: Duration,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct ScreenshotObserverState {
    pub last_screenshot_at_ms: Option<i64>,
    pub authenticated: bool,
}

pub struct ScreenshotObserver<C = ()> {
    pub state: ScreenshotObserverState,
    platform: Box<dyn ScreenshotHooks>,
    pub config: ScreenshotConfig,
    sender: Sender<Event<C>>,
}

impl<C: 'static> ScreenshotObserver<C> {
    pub fn new(
        platform: Box<dyn ScreenshotHooks>,
        sender: Sender<Event<C>>,
        config: ScreenshotConfig,
    ) -> Self {
        Self {
            state: ScreenshotObserverState::default(),
            platform,
            config,
            sender,
        }
    }
}

impl<C: 'static> Observer<C> for ScreenshotObserver<C> {
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
        if !self.state.authenticated {
            self.state.last_screenshot_at_ms = None;
        }
        Ok(())
    }

    fn on_event(&mut self, event: &Event<C>) -> CoreResult<()> {
        match event {
            Event::Login { .. } => {
                self.state.authenticated = true;
                self.state.last_screenshot_at_ms = None;
                return Ok(());
            }
            Event::Logout => {
                self.state.authenticated = false;
                self.state.last_screenshot_at_ms = None;
                return Ok(());
            }
            Event::Ping => {}
            _ => return Ok(()),
        }

        if !self.state.authenticated {
            return Ok(());
        }

        let now_ms = self.platform.get_time_utc_ms()?;
        let interval_ms = self.config.screenshot_interval.as_millis() as i64;

        // Sanity check: reset if time went backwards.
        if let Some(last) = self.state.last_screenshot_at_ms
            && now_ms < last
        {
            self.state.last_screenshot_at_ms = None;
        }

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
            Err(_) => {
                self.sender.send(Event::CaptureFailed).ok();
                return Ok(());
            }
        };
        let processed = image_pipeline::ImagePipeline.process(screenshot)?;
        self.state.last_screenshot_at_ms = Some(now_ms);
        self.sender
            .send(Event::Upload {
                risk: 0.0,
                kind: UploadKind::Screenshot {
                    image: processed.bytes,
                    content_type: processed.content_type,
                },
            })
            .ok();
        Ok(())
    }
}
