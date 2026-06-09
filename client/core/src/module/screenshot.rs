pub mod image_pipeline;

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};
use crate::events::types::{CaptureFailed, ConfigChanged, Login, Logout, Ping, Upload};
use crate::model::UploadKind;
use crate::platform::ScreenshotHooks;

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct ScreenshotObserverState {
    pub last_screenshot_at_ms: Option<i64>,
    pub authenticated: bool,
}

pub struct ScreenshotInner {
    pub state: ScreenshotObserverState,
    platform: Box<dyn ScreenshotHooks>,
    pub screenshot_interval_ms: i64,
}

impl ScreenshotInner {
    fn handle_ping(&mut self, emitter: &Emitter) -> CoreResult<()> {
        if !self.state.authenticated {
            return Ok(());
        }

        let now_ms = self.platform.get_time_utc_ms()?;

        // Sanity check: reset if time went backwards.
        if let Some(last) = self.state.last_screenshot_at_ms {
            if now_ms < last {
                self.state.last_screenshot_at_ms = None;
            }
        }

        let should = self
            .state
            .last_screenshot_at_ms
            .map(|last| now_ms - last >= self.screenshot_interval_ms)
            .unwrap_or(true);
        if !should {
            return Ok(());
        }

        let screenshot = match self.platform.take_screenshot() {
            Ok(s) => s,
            Err(_) => {
                let _ = emitter.send(CaptureFailed);
                return Ok(());
            }
        };
        let processed = image_pipeline::ImagePipeline.process(screenshot)?;
        self.state.last_screenshot_at_ms = Some(now_ms);
        let _ = emitter.send(Upload {
            risk: 0.0,
            kind: UploadKind::Screenshot {
                image: processed.bytes,
                content_type: processed.content_type,
            },
        });
        Ok(())
    }
}

pub struct ScreenshotModule {
    pub inner: Arc<Mutex<ScreenshotInner>>,
}

impl ScreenshotModule {
    pub fn new(platform: Box<dyn ScreenshotHooks>, screenshot_interval_ms: i64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ScreenshotInner {
                state: ScreenshotObserverState::default(),
                platform,
                screenshot_interval_ms,
            })),
        }
    }
}

impl Observer for ScreenshotModule {
    fn name(&self) -> &'static str {
        "screenshot"
    }

    fn init(&mut self, bus: &mut EventBus, state: StateType) -> CoreResult<()> {
        if !state.is_null() {
            let mut g = self.inner.lock().unwrap();
            g.state = serde_json::from_value(state)?;
            if !g.state.authenticated {
                g.state.last_screenshot_at_ms = None;
            }
        }

        let emitter = bus.emitter();

        let inner = Arc::clone(&self.inner);
        bus.subscribe(move |_: &Login| {
            let mut g = inner.lock().unwrap();
            g.state.authenticated = true;
            g.state.last_screenshot_at_ms = None;
            Ok(())
        });

        let inner = Arc::clone(&self.inner);
        bus.subscribe(move |_: &Logout| {
            let mut g = inner.lock().unwrap();
            g.state.authenticated = false;
            g.state.last_screenshot_at_ms = None;
            Ok(())
        });

        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |_: &Ping| inner.lock().unwrap().handle_ping(&e));

        let inner = Arc::clone(&self.inner);
        bus.subscribe(move |ev: &ConfigChanged| {
            inner.lock().unwrap().screenshot_interval_ms = ev.screenshot_interval_ms as i64;
            Ok(())
        });

        Ok(())
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.inner.lock().unwrap().state)?)
    }
}
