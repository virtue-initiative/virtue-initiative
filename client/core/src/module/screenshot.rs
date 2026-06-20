pub mod fingerprint;
pub mod image_pipeline;
pub mod risk_classifier;

use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::events::Ping;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};
use crate::model::{ScreenshotSkipReason, UploadKind};
use crate::module::auth::{Login, Logout};
use crate::module::config::ConfigChanged;
use crate::module::upload::Upload;
use crate::platform::ScreenshotHooks;
use risk_classifier::RiskClassifier;

#[cfg(not(test))]
const MODEL_BYTES: &[u8] = include_bytes!("../../models/nsfw_small_v1.onnx");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureFailed;

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct ScreenshotObserverState {
    pub last_screenshot_at_ms: Option<i64>,
    pub enabled: bool,
    /// Grayscale grid fingerprint of the last frame we actually uploaded (size scales
    /// with the screen resolution; see [`fingerprint`]). Used to dedup: a capture whose
    /// fingerprint hasn't materially changed from this one is suppressed. Always compared
    /// against the last *uploaded* frame (never the previous capture) so cumulative
    /// sub-threshold drift eventually crosses the threshold and forces a fresh upload.
    #[serde(default, deserialize_with = "deserialize_fingerprint_lenient")]
    pub last_uploaded_fingerprint: Option<fingerprint::Fingerprint>,
}

/// Lenient deserializer for the dedup fingerprint: any value that doesn't match the current
/// [`fingerprint::Fingerprint`] shape — e.g. a fingerprint written by an older build whose
/// format has since changed — decodes to `None` instead of failing the whole observer's
/// state load. The fingerprint is only a dedup hint, so dropping it merely forces the next
/// frame to upload and re-baseline; that is far cheaper than failing `init` and crash-looping
/// the daemon on every start.
fn deserialize_fingerprint_lenient<'de, D>(
    deserializer: D,
) -> Result<Option<fingerprint::Fingerprint>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or(None))
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

        // Gate 1 — locked / screensaver: checked *before* capturing. While locked or
        // screensaving the user cannot be viewing real content, so skip the capture
        // entirely (saving capture + classification cost) and emit a lightweight
        // `ScreenshotSkipped` log so the feed records that monitoring was active. We
        // advance the cadence clock so pacing continues and we re-check next interval
        // (skip events fire at most once per screenshot interval); the last-uploaded
        // fingerprint is left untouched. Fail-safe is `false` (fall back to the diff
        // gate), never silently suppress.
        if self.platform.is_locked_or_screensaver()? {
            self.state.last_screenshot_at_ms = Some(now_ms);
            let _ = emitter.send(Upload {
                risk: 0.0,
                kind: UploadKind::ScreenshotSkipped {
                    reason: ScreenshotSkipReason::LockedOrScreensaver,
                },
            });
            return Ok(());
        }

        let screenshot = match self.platform.take_screenshot() {
            Ok(s) => s,
            Err(_) => {
                let _ = emitter.send(CaptureFailed);
                return Ok(());
            }
        };
        self.state.last_screenshot_at_ms = Some(now_ms);

        // Gate 2 — screen-change diff vs the last *uploaded* frame. A failed fingerprint
        // is `None`, which falls through to the upload path (fail-safe to upload). With no
        // prior uploaded fingerprint we always upload the first frame.
        let fingerprint = fingerprint::fingerprint(&screenshot.bytes).ok();
        let static_frame = match (
            self.state.last_uploaded_fingerprint.as_ref(),
            fingerprint.as_ref(),
        ) {
            (Some(prev), Some(cur)) => !fingerprint::changed(prev, cur),
            _ => false,
        };
        if static_frame {
            // Redundant frame: suppress image upload + classification, keep the anchor
            // fingerprint, but record a lightweight `ScreenshotSkipped` log so the feed
            // shows the screen was static rather than leaving an unexplained gap.
            let _ = emitter.send(Upload {
                risk: 0.0,
                kind: UploadKind::ScreenshotSkipped {
                    reason: ScreenshotSkipReason::StaticScreen,
                },
            });
            return Ok(());
        }

        let risk = self
            .classifier
            .as_ref()
            .and_then(|c| c.classify(&screenshot.bytes).ok())
            .unwrap_or(0.0);
        let processed = image_pipeline::ImagePipeline.process(screenshot)?;
        let _ = emitter.send(Upload {
            risk,
            kind: UploadKind::Screenshot {
                image: processed.bytes,
                content_type: processed.content_type,
            },
        });
        if let Some(fingerprint) = fingerprint {
            self.state.last_uploaded_fingerprint = Some(fingerprint);
        }
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
                Ok(())
            },
            _: Logout => {
                self.state.enabled = false;
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
    use crate::model::{
        BatchRecipient, DeviceCredentials, DeviceSettings, ScreenshotSkipReason, UploadKind,
    };
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
    fn static_frame_suppressed_after_first_upload() {
        use crate::testing::fixtures::solid_png_screenshot;
        let mut b = EventTester::builder();
        b.platform()
            .set_default_screenshot(solid_png_screenshot(100));
        b.add(ScreenshotModule::new(Box::new(b.platform()), 60_000));
        let mut t = b.build();
        t.emit(1, test_login());
        t.clear_captured();
        // First ping uploads the baseline frame.
        t.emit(1, Ping);
        assert_eq!(t.captured::<Upload>().len(), 1);
        t.clear_captured();
        // Next interval: identical screen → captured but image upload suppressed; a
        // `ScreenshotSkipped { StaticScreen }` log is emitted instead.
        t.emit(61, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::ScreenshotSkipped {
                reason: ScreenshotSkipReason::StaticScreen
            },
            ..
        }));
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::Screenshot { .. },
            ..
        }));
        assert_eq!(t.platform.take_call_count(), 2);
    }

    #[test]
    fn changed_frame_uploads_again() {
        use crate::testing::fixtures::solid_png_screenshot;
        let mut b = EventTester::builder();
        b.platform()
            .set_default_screenshot(solid_png_screenshot(100));
        b.add(ScreenshotModule::new(Box::new(b.platform()), 60_000));
        let mut t = b.build();
        t.emit(1, test_login());
        t.emit(1, Ping); // baseline upload (solid 100)
        t.clear_captured();
        // A materially different frame on the next capture must re-upload.
        t.platform.queue_screenshot(Ok(solid_png_screenshot(220)));
        t.emit(61, Ping);
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Screenshot { .. },
            ..
        }));
    }

    #[test]
    fn locked_skips_capture_entirely() {
        let mut b = EventTester::builder();
        b.platform().set_locked_or_screensaver(true);
        let mut module = ScreenshotModule::new(Box::new(b.platform()), 60_000);
        module.state.enabled = true;
        b.add(module);
        let mut t = b.build();
        t.emit(1, Ping);
        // No image captured, but a `ScreenshotSkipped { LockedOrScreensaver }` is logged.
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::ScreenshotSkipped {
                reason: ScreenshotSkipReason::LockedOrScreensaver
            },
            ..
        }));
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::Screenshot { .. },
            ..
        }));
        // No capture taken at all while locked.
        assert_eq!(t.platform.take_call_count(), 0);
        // Cadence still advances so we re-check next interval.
        assert_eq!(
            t.observer::<ScreenshotModule>().state.last_screenshot_at_ms,
            Some(1000)
        );
    }

    #[test]
    fn unlock_resumes_capture() {
        let mut b = EventTester::builder();
        b.platform().set_locked_or_screensaver(true);
        let mut module = ScreenshotModule::new(Box::new(b.platform()), 60_000);
        module.state.enabled = true;
        b.add(module);
        let mut t = b.build();
        t.emit(1, Ping); // locked → skip (records ScreenshotSkipped, no Screenshot)
        t.assert_not_like(crate::like!(Upload {
            kind: UploadKind::Screenshot { .. },
            ..
        }));
        t.clear_captured();
        t.platform.set_locked_or_screensaver(false);
        t.emit(61, Ping); // unlocked → capture + upload
        t.assert_like(crate::like!(Upload {
            kind: UploadKind::Screenshot { .. },
            ..
        }));
        assert_eq!(t.platform.take_call_count(), 1);
    }

    #[test]
    fn legacy_fingerprint_format_does_not_break_state_load() {
        // An older build persisted `last_uploaded_fingerprint` as a flat integer array;
        // the current type is a struct. Loading such state must not fail — the fingerprint
        // resets to None while the rest of the screenshot state is preserved (rather than
        // erroring out of `init` and crash-looping the daemon).
        let saved = serde_json::json!({
            "screenshot": {
                "enabled": true,
                "last_screenshot_at_ms": 12_345,
                "last_uploaded_fingerprint": [22, 17, 17, 19, 23]
            }
        });
        let mut b = EventTester::builder();
        b.add(ScreenshotModule::new(Box::new(b.platform()), 60_000));
        b.with_state(saved);
        let mut t = b.build();
        let st = &t.observer::<ScreenshotModule>().state;
        assert!(st.enabled, "enabled flag should survive the load");
        assert_eq!(st.last_screenshot_at_ms, Some(12_345));
        assert!(
            st.last_uploaded_fingerprint.is_none(),
            "incompatible legacy fingerprint should decode to None"
        );
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
