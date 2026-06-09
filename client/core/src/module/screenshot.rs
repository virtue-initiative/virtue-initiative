pub mod image_pipeline;

use std::any::Any;

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

pub struct ScreenshotModule {
    pub state: ScreenshotObserverState,
    platform: Box<dyn ScreenshotHooks>,
    pub screenshot_interval_ms: i64,
}

impl ScreenshotModule {
    pub fn new(platform: Box<dyn ScreenshotHooks>, screenshot_interval_ms: i64) -> Self {
        Self {
            state: ScreenshotObserverState::default(),
            platform,
            screenshot_interval_ms,
        }
    }

    fn handle_ping(&mut self, emitter: &Emitter) -> CoreResult<()> {
        if !self.state.authenticated {
            return Ok(());
        }

        let now_ms = self.platform.get_time_utc_ms()?;

        // Sanity check: reset if time went backwards.
        if let Some(last) = self.state.last_screenshot_at_ms
            && now_ms < last
        {
            self.state.last_screenshot_at_ms = None;
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

impl Observer for ScreenshotModule {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "screenshot"
    }

    fn init(&mut self, _bus: &mut EventBus, state: StateType) -> CoreResult<()> {
        if !state.is_null() {
            self.state = serde_json::from_value(state)?;
            if !self.state.authenticated {
                self.state.last_screenshot_at_ms = None;
            }
        }
        Ok(())
    }

    fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
        crate::dispatch_event!(event, {
            _: Login => {
                self.state.authenticated = true;
                self.state.last_screenshot_at_ms = None;
                Ok(())
            },
            _: Logout => {
                self.state.authenticated = false;
                self.state.last_screenshot_at_ms = None;
                Ok(())
            },
            _: Ping => self.handle_ping(emitter),
            ev: ConfigChanged => {
                self.screenshot_interval_ms = ev.screenshot_interval_ms as i64;
                Ok(())
            },
        })
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.state)?)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::ScreenshotModule;
    use crate::events::bus::{EventBus, StateType};
    use crate::events::types::{Login, Logout, Ping, Upload};
    use crate::model::{BatchRecipient, DeviceCredentials, DeviceSettings, UploadKind};
    use crate::testing::TestPlatformHooks;

    fn valid_credentials() -> DeviceCredentials {
        DeviceCredentials {
            device_id: "test-device".into(),
            access_token: "test-access".into(),
            refresh_token: "test-refresh".into(),
        }
    }

    fn valid_settings() -> DeviceSettings {
        DeviceSettings {
            device_id: "test-device".into(),
            name: "test device".into(),
            platform: "test".into(),
            owner: Some(BatchRecipient {
                user_id: "test-user".into(),
                pub_key_base64: "CQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            }),
            partners: Vec::new(),
            hash_base_url: None,
        }
    }

    fn make(ts: i64) -> (EventBus, Arc<Mutex<Vec<Upload>>>, TestPlatformHooks) {
        let platform = TestPlatformHooks::new();
        platform.clock.set(ts);
        let platform_clone = platform.clone();
        let module = ScreenshotModule::new(Box::new(platform), 60_000);
        let uploads: Arc<Mutex<Vec<Upload>>> = Arc::new(Mutex::new(Vec::new()));
        let u = Arc::clone(&uploads);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &Upload| {
            u.lock().unwrap().push(ev.clone());
            Ok(())
        });
        (bus, uploads, platform_clone)
    }

    #[test]
    fn ping_when_unauthenticated_does_nothing() {
        let (mut bus, uploads, platform) = make(1_000);
        bus.send(Ping).unwrap();
        bus.iter().unwrap();
        assert!(uploads.lock().unwrap().is_empty());
        assert_eq!(platform.take_call_count(), 0);
    }

    #[test]
    fn login_then_ping_takes_screenshot() {
        let (mut bus, uploads, platform) = make(1_000);
        bus.send(Login {
            credentials: valid_credentials(),
            settings: valid_settings(),
        })
        .unwrap();
        bus.iter().unwrap();
        uploads.lock().unwrap().clear();

        bus.send(Ping).unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter()
                .any(|e| matches!(e.kind, UploadKind::Screenshot { .. })),
            "expected Screenshot upload after first ping post-login"
        );
        assert_eq!(platform.take_call_count(), 1);
    }

    #[test]
    fn screenshot_not_retaken_before_interval() {
        let platform = TestPlatformHooks::new();
        platform.clock.set(30_000);
        let mut module = ScreenshotModule::new(Box::new(platform.clone()), 60_000);
        module.state.authenticated = true;
        module.state.last_screenshot_at_ms = Some(0);

        let uploads: Arc<Mutex<Vec<Upload>>> = Arc::new(Mutex::new(Vec::new()));
        let u = Arc::clone(&uploads);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &Upload| {
            u.lock().unwrap().push(ev.clone());
            Ok(())
        });

        bus.send(Ping).unwrap();
        bus.iter().unwrap();
        assert_eq!(platform.take_call_count(), 0);
    }

    #[test]
    fn screenshot_retaken_after_interval() {
        let platform = TestPlatformHooks::new();
        platform.clock.set(61_000);
        let mut module = ScreenshotModule::new(Box::new(platform.clone()), 60_000);
        module.state.authenticated = true;
        module.state.last_screenshot_at_ms = Some(0);

        let uploads: Arc<Mutex<Vec<Upload>>> = Arc::new(Mutex::new(Vec::new()));
        let u = Arc::clone(&uploads);
        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.subscribe(move |ev: &Upload| {
            u.lock().unwrap().push(ev.clone());
            Ok(())
        });

        bus.send(Ping).unwrap();
        bus.iter().unwrap();
        let u = uploads.lock().unwrap();
        assert!(
            u.iter()
                .any(|e| matches!(e.kind, UploadKind::Screenshot { .. })),
            "expected screenshot after interval elapsed"
        );
        assert_eq!(platform.take_call_count(), 1);
    }

    #[test]
    fn logout_clears_authenticated_and_schedule() {
        let platform = TestPlatformHooks::new();
        let mut module = ScreenshotModule::new(Box::new(platform), 60_000);
        module.state.authenticated = true;
        module.state.last_screenshot_at_ms = Some(500);

        let mut bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        bus.send(Logout).unwrap();
        bus.iter().unwrap();

        let m = bus
            .observer_mut("screenshot")
            .unwrap()
            .as_any_mut()
            .downcast_mut::<ScreenshotModule>()
            .unwrap();
        assert!(!m.state.authenticated);
        assert_eq!(m.state.last_screenshot_at_ms, None);
    }
}
