pub mod image_pipeline;

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::Event;
use crate::crypto::prepare_screenshot_event;
use crate::platform::PlatformHooks;

pub struct ScreenshotConfig {
    pub screenshot_interval: Duration,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct ScreenshotObserverState {
    pub last_screenshot_at_ms: Option<i64>,
}

pub struct ScreenshotObserver<P: PlatformHooks> {
    pub state: ScreenshotObserverState,
    platform: P,
    pub config: ScreenshotConfig,
}

impl<P: PlatformHooks> ScreenshotObserver<P> {
    pub fn new(state: ScreenshotObserverState, platform: P, config: ScreenshotConfig) -> Self {
        Self {
            state,
            platform,
            config,
        }
    }

    pub fn reset_schedule(&mut self) {
        self.state.last_screenshot_at_ms = None;
    }

    pub(super) fn on_event(
        &mut self,
        event: &Event,
        _now_ms: i64,
    ) -> crate::error::CoreResult<Vec<Event>> {
        match event {
            Event::Tick { now_ms } => self.handle_tick(*now_ms),
            _ => Ok(vec![]),
        }
    }

    fn handle_tick(&mut self, now_ms: i64) -> crate::error::CoreResult<Vec<Event>> {
        let interval_ms = self.config.screenshot_interval.as_millis() as i64;
        let should = self
            .state
            .last_screenshot_at_ms
            .map(|last| now_ms - last >= interval_ms)
            .unwrap_or(true);
        if !should {
            return Ok(vec![]);
        }
        let screenshot = self.platform.take_screenshot()?;
        let processed = image_pipeline::ImagePipeline.process(screenshot)?;
        let data = prepare_screenshot_event(processed)?;
        self.state.last_screenshot_at_ms = Some(now_ms);
        Ok(vec![Event::ScreenshotCaptured { data }])
    }
}
