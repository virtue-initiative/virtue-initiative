pub mod image_pipeline;
pub mod risk_classifier;

use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::LifecycleKind;
use crate::error::CoreResult;
use crate::events::Ping;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};
use crate::model::UploadKind;
use crate::module::auth::{Login, Logout};
use crate::module::config::ConfigChanged;
use crate::module::upload::Upload;
use crate::platform::ScreenshotHooks;
use risk_classifier::RiskClassifier;

#[cfg(not(test))]
const MODEL_BYTES: &[u8] = include_bytes!("../../models/nsfw_small_v1.onnx");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureFailed;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotPaused;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotResumed;

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct ScreenshotObserverState {
    pub last_screenshot_at_ms: Option<i64>,
    pub enabled: bool,
}

pub struct ScreenshotModule {
    pub state: ScreenshotObserverState,
    platform: Box<dyn ScreenshotHooks>,
    pub screenshot_interval_ms: i64,
    classifier: Option<RiskClassifier>,
}

impl ScreenshotModule {
    pub fn new(platform: Box<dyn ScreenshotHooks>, screenshot_interval_ms: i64) -> Self {
        #[cfg(not(test))]
        let classifier = RiskClassifier::new(MODEL_BYTES).ok();
        #[cfg(test)]
        let classifier: Option<RiskClassifier> = None;
        Self {
            state: ScreenshotObserverState::default(),
            platform,
            screenshot_interval_ms,
            classifier,
        }
    }

    fn handle_ping(&mut self, emitter: &Emitter) -> CoreResult<()> {
        if !self.state.enabled {
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
        let risk = self
            .classifier
            .as_ref()
            .and_then(|c| c.classify(&screenshot.bytes).ok())
            .unwrap_or(0.0);
        let processed = image_pipeline::ImagePipeline.process(screenshot)?;
        self.state.last_screenshot_at_ms = Some(now_ms);
        let _ = emitter.send(Upload {
            risk,
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
            if !self.state.enabled {
                self.state.last_screenshot_at_ms = None;
            }
        }
        Ok(())
    }

    fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
        crate::dispatch_event!(event, {
            _: Login => {
                self.state.enabled = true;
                self.state.last_screenshot_at_ms = None;
                emitter.send(Upload {
                    risk: 0.0,
                    kind: UploadKind::Lifecycle { kind: LifecycleKind::ScreenshotResumed },
                })?;
                Ok(())
            },
            _: Logout => {
                self.state.enabled = false;
                self.state.last_screenshot_at_ms = None;
                emitter.send(Upload {
                    risk: 0.0,
                    kind: UploadKind::Lifecycle { kind: LifecycleKind::ScreenshotPaused },
                })?;
                Ok(())
            },
            _: ScreenshotPaused => {
                self.state.enabled = false;
                self.state.last_screenshot_at_ms = None;
                Ok(())
            },
            _: ScreenshotResumed => {
                self.state.enabled = true;
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
    use super::*;
    use crate::events::Ping;
    use crate::model::{BatchRecipient, DeviceCredentials, DeviceSettings, UploadKind};
    use crate::module::auth::{Login, Logout};
    use crate::module::upload::Upload;
    use crate::testing::EventTester;

    fn test_login() -> Login {
        Login {
            credentials: DeviceCredentials {
                device_id: "test-device".into(),
                access_token: "test-access".into(),
                refresh_token: "test-refresh".into(),
            },
            settings: DeviceSettings {
                device_id: "test-device".into(),
                name: "test device".into(),
                platform: "test".into(),
                owner: Some(BatchRecipient {
                    user_id: "test-user".into(),
                    pub_key_base64: "CQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
                }),
                partners: Vec::new(),
                hash_base_url: None,
            },
        }
    }

    #[test]
    fn ping_when_logged_out_does_nothing() {
        let mut b = EventTester::builder();
        b.add(ScreenshotModule::new(Box::new(b.platform()), 60_000));
        let mut t = b.build();
        t.emit(1, Ping);
        assert!(t.captured::<Upload>().is_empty());
        assert_eq!(t.platform.take_call_count(), 0);
    }

    #[test]
    fn login_then_ping_takes_screenshot() {
        let mut b = EventTester::builder();
        b.add(ScreenshotModule::new(Box::new(b.platform()), 60_000));
        let mut t = b.build();
        t.emit(1, test_login());
        t.clear_captured();
        t.emit(1, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Screenshot { .. },
            ..
        }));
        assert_eq!(t.platform.take_call_count(), 1);
    }

    #[test]
    fn screenshot_not_retaken_before_interval() {
        let mut b = EventTester::builder();
        b.clock.set(30_000);
        let mut module = ScreenshotModule::new(Box::new(b.platform()), 60_000);
        module.state.enabled = true;
        module.state.last_screenshot_at_ms = Some(0);
        b.add(module);
        let mut t = b.build();
        t.emit(30, Ping);
        assert_eq!(t.platform.take_call_count(), 0);
    }

    #[test]
    fn screenshot_retaken_after_interval() {
        let mut b = EventTester::builder();
        b.clock.set(61_000);
        let mut module = ScreenshotModule::new(Box::new(b.platform()), 60_000);
        module.state.enabled = true;
        module.state.last_screenshot_at_ms = Some(0);
        b.add(module);
        let mut t = b.build();
        t.emit(61, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Screenshot { .. },
            ..
        }));
        assert_eq!(t.platform.take_call_count(), 1);
    }

    #[test]
    fn logout_disables_and_resets_schedule() {
        let mut b = EventTester::builder();
        let mut module = ScreenshotModule::new(Box::new(b.platform()), 60_000);
        module.state.enabled = true;
        module.state.last_screenshot_at_ms = Some(500);
        b.add(module);
        let mut t = b.build();
        t.emit(1, Logout);
        assert!(!t.observer::<ScreenshotModule>().state.enabled);
        assert_eq!(
            t.observer::<ScreenshotModule>().state.last_screenshot_at_ms,
            None
        );
    }
}
