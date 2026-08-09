pub mod fingerprint;
pub mod image_pipeline;
pub mod risk_classifier;

use std::any::Any;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ScreenshotHooks;
use crate::error::CoreResult;
use crate::events::Ping;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};
use crate::model::{ScreenshotSkipReason, UploadKind};
use crate::module::auth::{Login, Logout};
use crate::module::config::ConfigChanged;
use crate::module::upload::Upload;
use risk_classifier::RiskClassifier;
use virtue_text_detection::ScreenshotOCR;

#[cfg(not(test))]
const MODEL_BYTES: &[u8] = include_bytes!("../../models/nsfw_small_v1.onnx");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureFailed;

/// In-process event emitted by the background capture job when it finishes (on
/// **every** branch, including failure) so the module can clear its in-flight
/// guard. Never serialized to the network — it only travels the local bus.
///
/// `update_fingerprint` is `Some` only when a frame was actually uploaded and
/// its fingerprint computed; the module then advances its dedup anchor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotCaptured {
    pub update_fingerprint: Option<fingerprint::Fingerprint>,
}

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
    /// Shared so a background capture job can hold its own handle (`Arc`, no
    /// `Mutex` — `ScreenshotHooks` is `Send + Sync`).
    platform: Arc<dyn ScreenshotHooks>,
    pub screenshot_interval_ms: i64,
    /// Shared with the background capture job. `RiskClassifier` is `Send + Sync`
    /// (tract op types are), so an `Arc` crosses threads without a `Mutex`.
    classifier: Option<Arc<RiskClassifier>>,
    /// Shared OCR engine for text redaction before upload. `None` when OCR
    /// is unavailable (platform not supported, tesseract not installed, etc.).
    ocr: Option<Arc<ScreenshotOCR>>,
    /// True while a background capture is running. A **module field**, never
    /// persisted to `event_state.json`, so a crash/restart can't leave it stuck
    /// true. Guards against launching overlapping captures when one runs longer
    /// than the screenshot interval.
    capture_in_flight: bool,
}

impl ScreenshotModule {
    pub fn new(platform: Arc<dyn ScreenshotHooks>, screenshot_interval_ms: i64) -> Self {
        #[cfg(not(test))]
        let classifier = match RiskClassifier::new(MODEL_BYTES) {
            Ok(classifier) => Some(Arc::new(classifier)),
            Err(err) => {
                // The model is embedded via `include_bytes!` at build time; the usual cause of
                // a load failure is an unresolved Git LFS pointer baked in instead of the real
                // ONNX (see build.rs guard). Without a classifier every screenshot risk is 0,
                // so make that loud rather than silent.
                tracing::error!(
                    error = %err,
                    "NSFW classifier disabled, all screenshot risk will be 0"
                );
                None
            }
        };
        #[cfg(test)]
        let classifier: Option<Arc<RiskClassifier>> = None;

        #[cfg(not(test))]
        let ocr = match ScreenshotOCR::new(Default::default()) {
            Ok(ocr) => Some(Arc::new(ocr)),
            Err(err) => {
                tracing::warn!(error = %err, "OCR disabled, text will not be redacted");
                None
            }
        };
        #[cfg(test)]
        let ocr: Option<Arc<ScreenshotOCR>> = None;

        Self {
            state: ScreenshotObserverState::default(),
            platform,
            screenshot_interval_ms,
            classifier,
            ocr,
            capture_in_flight: false,
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

        // Gate 1 — locked / screensaver: checked *before* capturing (inline, cheap).
        // While locked or screensaving the user cannot be viewing real content, so skip
        // the capture entirely (saving capture + classification cost) and emit a
        // lightweight `ScreenshotSkipped` log so the feed records that monitoring was
        // active. We advance the cadence clock so pacing continues and we re-check next
        // interval (skip events fire at most once per screenshot interval); the
        // last-uploaded fingerprint is left untouched. No background work is spawned.
        // Fail-safe is `false` (fall back to the diff gate), never silently suppress.
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

        // Overlap guard: a previous capture is still running on a background thread.
        // Skip this interval rather than stacking captures (covers captures slower than
        // the screenshot interval).
        if self.capture_in_flight {
            return Ok(());
        }

        // Hand the heavy work (capture → fingerprint → classify → image-process) to a
        // background thread so a slow capture can't stall the daemon's event loop and
        // trip a false `UnexpectedGap`. Advance the cadence clock and set the
        // in-flight guard *before* spawning so pacing continues while it runs; the guard
        // is cleared when the terminal `ScreenshotCaptured` event arrives.
        self.capture_in_flight = true;
        self.state.last_screenshot_at_ms = Some(now_ms);

        let platform = Arc::clone(&self.platform);
        let classifier = self.classifier.clone();
        let ocr = self.ocr.clone();
        let anchor = self.state.last_uploaded_fingerprint.clone();
        emitter.spawn(move |em| {
            run_capture(
                platform.as_ref(),
                classifier.as_deref(),
                ocr.as_deref(),
                anchor.as_ref(),
                em,
            )
        });

        Ok(())
    }
}

/// Heavy screenshot pipeline run off the event loop (capture → fingerprint diff →
/// classify → image process). Thread-free and unit-testable on its own.
///
/// Emits **exactly one** [`ScreenshotCaptured`] on every branch so the module's
/// in-flight guard always clears, plus the appropriate payload:
/// - capture error → [`CaptureFailed`] + `ScreenshotCaptured { None }`,
/// - static vs anchor → `Upload { ScreenshotSkipped { StaticScreen } }` + `{ None }`,
/// - else → `Upload { Screenshot { .. } }` + `ScreenshotCaptured { Some(fp) }` (`Some`
///   only when the fingerprint computed, matching the prior dedup behavior).
fn run_capture(
    platform: &dyn ScreenshotHooks,
    classifier: Option<&RiskClassifier>,
    ocr: Option<&ScreenshotOCR>,
    anchor: Option<&fingerprint::Fingerprint>,
    emitter: &Emitter,
) -> CoreResult<()> {
    let screenshot = match platform.take_screenshot() {
        Ok(s) => s,
        Err(_) => {
            let _ = emitter.send(CaptureFailed);
            let _ = emitter.send(ScreenshotCaptured {
                update_fingerprint: None,
            });
            return Ok(());
        }
    };

    // Gate 2 — screen-change diff vs the last *uploaded* frame. A failed fingerprint
    // is `None`, which falls through to the upload path (fail-safe to upload). With no
    // prior uploaded fingerprint we always upload the first frame.
    let fingerprint = fingerprint::fingerprint(&screenshot.bytes).ok();
    let static_frame = match (anchor, fingerprint.as_ref()) {
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
        let _ = emitter.send(ScreenshotCaptured {
            update_fingerprint: None,
        });
        return Ok(());
    }

    // Classify before the (consuming) image pipeline. A `None` classifier (model failed to
    // load) or a classify error fails safe to risk 0 with no raw scores, but — unlike the old
    // silent `.ok()` — we log the error so an always-0 misconfiguration is diagnosable.
    let scores = classifier.and_then(|c| match c.classify(&screenshot.bytes) {
        Ok(scores) => Some(scores),
        Err(err) => {
            tracing::warn!(error = %err, "classify failed, recording risk 0");
            None
        }
    });
    let (risk, skin_detection, nsfw_detection) = match scores {
        Some(scores) => (scores.risk, Some(scores.skin), scores.nsfw),
        None => (0.0, None, None),
    };
    let screenshot = redact_if_ocr(ocr, screenshot);
    let processed = match image_pipeline::ImagePipeline.process(screenshot) {
        Ok(p) => p,
        Err(err) => {
            // Clear the in-flight guard even when image processing fails, then let the
            // error propagate (the bus turns it into an `Error` event).
            let _ = emitter.send(ScreenshotCaptured {
                update_fingerprint: None,
            });
            return Err(err);
        }
    };
    let _ = emitter.send(Upload {
        risk,
        kind: UploadKind::Screenshot {
            image: processed.bytes,
            content_type: processed.content_type,
            skin_detection,
            nsfw_detection,
        },
    });
    let _ = emitter.send(ScreenshotCaptured {
        update_fingerprint: fingerprint,
    });
    Ok(())
}

fn redact_if_ocr(ocr: Option<&ScreenshotOCR>, mut shot: crate::Screenshot) -> crate::Screenshot {
    let Some(engine) = ocr else {
        return shot;
    };
    match engine.detect(&shot.bytes) {
        Ok(result) if !result.regions.is_empty() => {
            if let Err(err) = redact_text_regions(&mut shot, &result.regions) {
                tracing::warn!(error = %err, "text redaction failed, uploading unredacted");
            }
            shot
        }
        Ok(_) => shot,
        Err(err) => {
            tracing::warn!(error = %err, "OCR failed, uploading unredacted");
            shot
        }
    }
}

fn redact_text_regions(
    shot: &mut crate::Screenshot,
    regions: &[virtue_text_detection::TextRegion],
) -> CoreResult<()> {
    let mut img =
        image::load_from_memory_with_format(&shot.bytes, image::ImageFormat::Png)?.to_rgba8();
    for r in regions {
        let bb = &r.bounding_box;
        for py in (bb.y as u32)..((bb.y + bb.height) as u32).min(img.height()) {
            for px in (bb.x as u32)..((bb.x + bb.width) as u32).min(img.width()) {
                img.put_pixel(px, py, image::Rgba([0, 0, 0, 255]));
            }
        }
    }
    let dyn_img = image::DynamicImage::ImageRgba8(img);
    let mut out = Vec::new();
    dyn_img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)?;
    shot.bytes = out;
    Ok(())
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
            ev: ScreenshotCaptured => {
                self.capture_in_flight = false;
                if let Some(fp) = &ev.update_fingerprint {
                    self.state.last_uploaded_fingerprint = Some(fp.clone());
                }
                Ok(())
            },
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
                refresh_token: "test-refresh".into(),
                signing_key: [1u8; 32],
            },
            settings: DeviceSettings {
                device_id: "test-device".into(),
                name: "test device".into(),
                platform: "test".into(),
                wrapping_keys: vec![BatchRecipient {
                    user_id: "test-user".into(),
                    pub_key_base64: "CQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
                }],
                hash_base_url: None,
            },
            hash_token: "test-hash-token".into(),
        }
    }

    #[test]
    fn ping_when_logged_out_does_nothing() {
        let mut b = EventTester::builder();
        b.add(ScreenshotModule::new(Arc::new(b.platform()), 60_000));
        let mut t = b.build();
        t.emit(1, Ping);
        assert!(t.captured::<Upload>().is_empty());
        assert_eq!(t.platform.take_call_count(), 0);
    }

    #[test]
    fn login_then_ping_takes_screenshot() {
        let mut b = EventTester::builder();
        b.add(ScreenshotModule::new(Arc::new(b.platform()), 60_000));
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
        let mut module = ScreenshotModule::new(Arc::new(b.platform()), 60_000);
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
        let mut module = ScreenshotModule::new(Arc::new(b.platform()), 60_000);
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
        b.add(ScreenshotModule::new(Arc::new(b.platform()), 60_000));
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
        b.add(ScreenshotModule::new(Arc::new(b.platform()), 60_000));
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
        let mut module = ScreenshotModule::new(Arc::new(b.platform()), 60_000);
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
        let mut module = ScreenshotModule::new(Arc::new(b.platform()), 60_000);
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
        b.add(ScreenshotModule::new(Arc::new(b.platform()), 60_000));
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
        let mut module = ScreenshotModule::new(Arc::new(b.platform()), 60_000);
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

    #[test]
    fn capture_completion_updates_fingerprint_and_clears_flag() {
        use crate::testing::fixtures::solid_png_screenshot;
        let mut b = EventTester::builder();
        b.platform()
            .set_default_screenshot(solid_png_screenshot(100));
        b.add(ScreenshotModule::new(Arc::new(b.platform()), 60_000));
        let mut t = b.build();
        t.emit(1, test_login());
        t.emit(1, Ping);
        // The inline spawner runs the whole capture pipeline synchronously, so by now
        // the terminal `ScreenshotCaptured` has updated the dedup anchor and cleared
        // the in-flight guard.
        let m = t.observer::<ScreenshotModule>();
        assert!(
            m.state.last_uploaded_fingerprint.is_some(),
            "anchor fingerprint should be set after a successful upload"
        );
        assert!(!m.capture_in_flight, "in-flight guard should be cleared");
    }

    #[test]
    fn capture_failure_emits_capture_failed_and_clears_flag() {
        let mut b = EventTester::builder();
        b.add(ScreenshotModule::new(Arc::new(b.platform()), 60_000));
        b.capture::<CaptureFailed>();
        let mut t = b.build();
        t.emit(1, test_login());
        t.clear_captured();
        // Next capture fails; the failure path must still emit `CaptureFailed` and clear
        // the in-flight guard so future pings aren't permanently blocked.
        t.platform
            .queue_screenshot(Err(crate::error::CoreError::CommandFailed("boom".into())));
        t.emit(1, Ping);
        assert_eq!(t.captured::<CaptureFailed>().len(), 1);
        assert!(
            !t.observer::<ScreenshotModule>().capture_in_flight,
            "in-flight guard should clear even when capture fails"
        );
    }

    #[test]
    fn in_flight_guard_prevents_overlapping_captures() {
        use std::sync::{Arc, Mutex};

        use crate::events::Ping;
        use crate::events::bus::{EventBus, Spawner, StateType};
        use crate::testing::TestPlatformHooks;

        type Job = Box<dyn FnOnce() + Send + 'static>;

        // Spawner that holds jobs until explicitly run, so we can observe the module
        // while a capture is mid-flight.
        #[derive(Clone, Default)]
        struct DeferringSpawner {
            jobs: Arc<Mutex<Vec<Job>>>,
        }
        impl DeferringSpawner {
            fn pending(&self) -> usize {
                self.jobs.lock().unwrap().len()
            }
            fn run_all(&self) {
                let jobs: Vec<_> = std::mem::take(&mut *self.jobs.lock().unwrap());
                for job in jobs {
                    job();
                }
            }
        }
        impl Spawner for DeferringSpawner {
            fn spawn(&self, job: Job) {
                self.jobs.lock().unwrap().push(job);
            }
        }

        let spawner = DeferringSpawner::default();
        let platform = TestPlatformHooks::new();
        let mut module = ScreenshotModule::new(Arc::new(platform.clone()), 60_000);
        module.state.enabled = true;
        let mut bus = EventBus::with_spawner(
            vec![Box::new(module)],
            StateType::Null,
            Arc::new(spawner.clone()),
        )
        .unwrap();

        // First ping: schedules a capture job (deferred — capture hasn't run yet).
        platform.clock.set(0);
        bus.send(Ping).unwrap();
        bus.iter().unwrap();
        assert_eq!(spawner.pending(), 1);
        assert_eq!(
            platform.take_call_count(),
            0,
            "capture runs inside the deferred job, not on the event loop"
        );

        // Second ping a full interval later, while the first capture is still in flight:
        // the overlap guard must skip it — no second job scheduled.
        platform.clock.set(60_000);
        bus.send(Ping).unwrap();
        bus.iter().unwrap();
        assert_eq!(
            spawner.pending(),
            1,
            "overlap guard must prevent a second concurrent capture"
        );

        // Run the deferred capture and drain its completion event: exactly one capture.
        spawner.run_all();
        bus.iter().unwrap();
        assert_eq!(platform.take_call_count(), 1);
    }
}
