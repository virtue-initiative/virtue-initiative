pub mod fingerprint;
pub mod image_pipeline;
pub mod risk_classifier;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::model::{ScreenshotSkipReason, UploadKind};
use crate::module::capture_availability::{self, CaptureAvailabilityState};
use crate::module::upload::{self, UploadState};
use crate::platform::ScreenshotHooks;
use crate::rng::RandomSource;
use risk_classifier::RiskClassifier;
use virtue_text_detection::ScreenshotOCR;

#[cfg(not(test))]
const MODEL_BYTES: &[u8] = include_bytes!("../../models/nsfw_small_v1.onnx");

/// Lenient deserializer for the dedup fingerprint: any value that doesn't match the current
/// [`fingerprint::Fingerprint`] shape — e.g. a fingerprint written by an older build whose
/// format has since changed — decodes to `None` instead of failing the whole state load.
fn deserialize_fingerprint_lenient<'de, D>(
    deserializer: D,
) -> Result<Option<fingerprint::Fingerprint>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or(None))
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct ScreenshotState {
    /// Randomly-drawn (exponential inter-arrival) time of the next screenshot
    /// attempt. `None` means "take one immediately" — the state right after
    /// login, before the first draw.
    pub next_screenshot_at_ms: Option<i64>,
    pub enabled: bool,
    /// Grayscale grid fingerprint of the last frame we actually uploaded (size scales
    /// with the screen resolution; see [`fingerprint`]). Used to dedup: a capture whose
    /// fingerprint hasn't materially changed from this one is suppressed. Always compared
    /// against the last *uploaded* frame (never the previous capture) so cumulative
    /// sub-threshold drift eventually crosses the threshold and forces a fresh upload.
    #[serde(default, deserialize_with = "deserialize_fingerprint_lenient")]
    pub last_uploaded_fingerprint: Option<fingerprint::Fingerprint>,
}

/// Draws the next screenshot time via an exponential inter-arrival: every
/// second has the same chance of being chosen, averaging `mean_ms` apart. See
/// `client/core/SPEC.md` §3.
fn draw_next_screenshot_at_ms(now_ms: i64, mean_ms: i64, rng: &dyn RandomSource) -> i64 {
    let u = rng.uniform();
    let delay_ms = -(mean_ms as f64) * (1.0 - u).ln();
    now_ms + delay_ms.round() as i64
}

/// Enable screenshot capture on login: the first screenshot is taken
/// immediately, on the next tick.
pub fn enable(state: &mut ScreenshotState) {
    state.enabled = true;
    state.next_screenshot_at_ms = None;
}

/// Disable screenshot capture on logout.
pub fn disable(state: &mut ScreenshotState) {
    state.enabled = false;
    state.next_screenshot_at_ms = None;
}

/// What `plan` decided to do this tick — passed unlocked to `capture_and_process`.
pub struct CapturePlan {
    anchor: Option<fingerprint::Fingerprint>,
}

/// Phase 2a (cheap, still locked): decide whether a screenshot is due this
/// tick, applying the locked/screensaver gate. Draws and stores the next
/// scheduled time whenever a decision (capture or skip) is made, so pacing
/// continues either way.
pub fn plan(
    state: &mut ScreenshotState,
    upload: &mut UploadState,
    hooks: &dyn ScreenshotHooks,
    now_ms: i64,
    mean_interval_ms: i64,
    rng: &dyn RandomSource,
) -> CoreResult<Option<CapturePlan>> {
    if !state.enabled {
        return Ok(None);
    }

    let due = state
        .next_screenshot_at_ms
        .is_none_or(|next| now_ms >= next);
    if !due {
        return Ok(None);
    }

    // Gate 1 — locked / screensaver: checked before capturing (cheap). Fail-safe
    // to `false` (fall back to the diff gate), never silently suppress.
    if hooks.is_locked_or_screensaver()? {
        state.next_screenshot_at_ms =
            Some(draw_next_screenshot_at_ms(now_ms, mean_interval_ms, rng));
        upload::enqueue(
            upload,
            now_ms,
            0.0,
            UploadKind::ScreenshotSkipped {
                reason: ScreenshotSkipReason::LockedOrScreensaver,
            },
        );
        return Ok(None);
    }

    state.next_screenshot_at_ms = Some(draw_next_screenshot_at_ms(now_ms, mean_interval_ms, rng));
    Ok(Some(CapturePlan {
        anchor: state.last_uploaded_fingerprint.clone(),
    }))
}

/// Outcome of the heavy (unlocked) capture pipeline.
pub enum CaptureOutcome {
    Failed,
    StaticFrame,
    Uploaded {
        risk: f32,
        kind: UploadKind,
        fingerprint: Option<fingerprint::Fingerprint>,
    },
}

/// Phase 2b (slow, unlocked): capture -> fingerprint diff -> classify ->
/// redact -> image process. Thread-free and unit-testable on its own.
pub fn capture_and_process(
    plan: CapturePlan,
    hooks: &dyn ScreenshotHooks,
    classifier: Option<&RiskClassifier>,
    ocr: Option<&ScreenshotOCR>,
) -> CaptureOutcome {
    let screenshot = match hooks.take_screenshot() {
        Ok(s) => s,
        Err(_) => return CaptureOutcome::Failed,
    };

    // Gate 2 — screen-change diff vs the last *uploaded* frame. A failed fingerprint
    // is `None`, which falls through to the upload path (fail-safe to upload). With no
    // prior uploaded fingerprint we always upload the first frame.
    let fp = fingerprint::fingerprint(&screenshot.bytes).ok();
    let static_frame = match (plan.anchor.as_ref(), fp.as_ref()) {
        (Some(prev), Some(cur)) => !fingerprint::changed(prev, cur),
        _ => false,
    };
    if static_frame {
        return CaptureOutcome::StaticFrame;
    }

    // Classify before the (consuming) image pipeline. A `None` classifier (model
    // failed to load) or a classify error fails safe to risk 0 with no raw scores,
    // logged so an always-0 misconfiguration is diagnosable.
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
            tracing::error!(error = %err, "screenshot image pipeline failed");
            return CaptureOutcome::Failed;
        }
    };

    CaptureOutcome::Uploaded {
        risk,
        kind: UploadKind::Screenshot {
            image: processed.bytes,
            content_type: processed.content_type,
            skin_detection,
            nsfw_detection,
        },
        fingerprint: fp,
    }
}

/// Phase 2c (locked): apply the capture outcome.
pub fn commit(
    state: &mut ScreenshotState,
    upload: &mut UploadState,
    availability: &mut CaptureAvailabilityState,
    outcome: Option<CaptureOutcome>,
    now_ms: i64,
) {
    match outcome {
        None => {}
        Some(CaptureOutcome::Failed) => {
            capture_availability::note_failure(availability, now_ms);
        }
        Some(CaptureOutcome::StaticFrame) => {
            upload::enqueue(
                upload,
                now_ms,
                0.0,
                UploadKind::ScreenshotSkipped {
                    reason: ScreenshotSkipReason::StaticScreen,
                },
            );
        }
        Some(CaptureOutcome::Uploaded {
            risk,
            kind,
            fingerprint,
        }) => {
            upload::enqueue(upload, now_ms, risk, kind);
            if let Some(fp) = fingerprint {
                state.last_uploaded_fingerprint = Some(fp);
            }
        }
    }
}

fn redact_if_ocr(
    ocr: Option<&ScreenshotOCR>,
    shot: crate::model::Screenshot,
) -> crate::model::Screenshot {
    let Some(engine) = ocr else {
        return shot;
    };
    let mut shot = shot;
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
    shot: &mut crate::model::Screenshot,
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

/// Loads the embedded NSFW classifier model. `None` (with a loud log) if it
/// fails to load — every screenshot risk then reads 0 rather than crashing.
pub fn load_classifier() -> Option<RiskClassifier> {
    #[cfg(not(test))]
    {
        match RiskClassifier::new(MODEL_BYTES) {
            Ok(classifier) => Some(classifier),
            Err(err) => {
                tracing::error!(
                    error = %err,
                    "NSFW classifier disabled, all screenshot risk will be 0"
                );
                None
            }
        }
    }
    #[cfg(test)]
    {
        None
    }
}

/// Loads the OCR engine used to redact text before upload. `None` (with a
/// logged warning) if unavailable on this platform.
pub fn load_ocr() -> Option<ScreenshotOCR> {
    #[cfg(not(test))]
    {
        match ScreenshotOCR::new(Default::default()) {
            Ok(ocr) => Some(ocr),
            Err(err) => {
                tracing::warn!(error = %err, "OCR disabled, text will not be redacted");
                None
            }
        }
    }
    #[cfg(test)]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BatchRecipient, DeviceCredentials, DeviceSettings};
    use crate::testing::{TestPlatformHooks, TestRandomSource};

    #[allow(clippy::field_reassign_with_default)]
    fn authenticated_upload() -> UploadState {
        let mut upload = UploadState::default();
        upload.device_credentials = Some(DeviceCredentials {
            device_id: "d".into(),
            refresh_token: "r".into(),
        });
        upload.settings = Some(DeviceSettings {
            device_id: "d".into(),
            name: "n".into(),
            platform: "p".into(),
            wrapping_keys: vec![BatchRecipient {
                user_id: "u".into(),
                pub_key_base64: "CQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            }],
            hash_base_url: None,
        });
        upload
    }

    #[test]
    fn disabled_never_plans_a_capture() {
        let mut state = ScreenshotState::default();
        let mut upload = authenticated_upload();
        let hooks = TestPlatformHooks::new();
        let rng = TestRandomSource::new();
        let plan = plan(&mut state, &mut upload, &hooks, 1_000, 300_000, &rng).unwrap();
        assert!(plan.is_none());
    }

    #[test]
    fn enabled_with_no_prior_schedule_captures_immediately() {
        let mut state = ScreenshotState::default();
        enable(&mut state);
        let mut upload = authenticated_upload();
        let hooks = TestPlatformHooks::new();
        let rng = TestRandomSource::new();
        let plan = plan(&mut state, &mut upload, &hooks, 1_000, 300_000, &rng).unwrap();
        assert!(plan.is_some());
        assert!(state.next_screenshot_at_ms.is_some());
    }

    #[test]
    fn not_due_before_the_drawn_time() {
        let mut state = ScreenshotState::default();
        enable(&mut state);
        state.next_screenshot_at_ms = Some(60_000);
        let mut upload = authenticated_upload();
        let hooks = TestPlatformHooks::new();
        let rng = TestRandomSource::new();
        let plan = plan(&mut state, &mut upload, &hooks, 30_000, 300_000, &rng).unwrap();
        assert!(plan.is_none());
    }

    #[test]
    fn locked_screen_skips_capture_and_reschedules() {
        let mut state = ScreenshotState::default();
        enable(&mut state);
        let mut upload = authenticated_upload();
        let hooks = TestPlatformHooks::new();
        hooks.set_locked_or_screensaver(true);
        let rng = TestRandomSource::new();
        let plan = plan(&mut state, &mut upload, &hooks, 1_000, 300_000, &rng).unwrap();
        assert!(plan.is_none());
        assert!(state.next_screenshot_at_ms.is_some());
        assert!(upload.pending_hash_events.iter().any(|e| matches!(
            e.event,
            UploadKind::ScreenshotSkipped {
                reason: ScreenshotSkipReason::LockedOrScreensaver
            }
        )));
    }

    #[test]
    fn static_frame_is_suppressed_but_capture_failure_is_reported() {
        use crate::testing::fixtures::solid_png_screenshot;
        let hooks = TestPlatformHooks::new();
        hooks.set_default_screenshot(solid_png_screenshot(100));
        let anchor = fingerprint::fingerprint(&solid_png_screenshot(100).bytes).unwrap();
        let outcome = capture_and_process(
            CapturePlan {
                anchor: Some(anchor),
            },
            &hooks,
            None,
            None,
        );
        assert!(matches!(outcome, CaptureOutcome::StaticFrame));
    }

    #[test]
    fn changed_frame_uploads() {
        use crate::testing::fixtures::solid_png_screenshot;
        let hooks = TestPlatformHooks::new();
        hooks.set_default_screenshot(solid_png_screenshot(220));
        let anchor = fingerprint::fingerprint(&solid_png_screenshot(10).bytes).unwrap();
        let outcome = capture_and_process(
            CapturePlan {
                anchor: Some(anchor),
            },
            &hooks,
            None,
            None,
        );
        assert!(matches!(outcome, CaptureOutcome::Uploaded { .. }));
    }

    #[test]
    fn capture_failure_reported() {
        let hooks = TestPlatformHooks::new();
        hooks.queue_screenshot(Err(crate::error::CoreError::CommandFailed("boom".into())));
        let outcome = capture_and_process(CapturePlan { anchor: None }, &hooks, None, None);
        assert!(matches!(outcome, CaptureOutcome::Failed));
    }

    #[test]
    fn draw_next_screenshot_is_deterministic_given_a_fixed_uniform() {
        let rng = TestRandomSource::new();
        rng.queue(0.5);
        let next = draw_next_screenshot_at_ms(0, 300_000, &rng);
        let expected = (-(300_000f64) * (0.5f64).ln()).round() as i64;
        assert_eq!(next, expected);
        assert!(next > 0);
    }
}
