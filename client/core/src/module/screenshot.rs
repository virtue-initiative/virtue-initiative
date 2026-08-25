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
const MODEL_BYTES: &[u8] = include_bytes!("../../models/nsfw_small_v1.nnef.tar");

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
/// CORE-003.
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

/// What `plan` decided to do this tick — passed to `capture_and_process`.
pub struct CapturePlan {
    anchor: Option<fingerprint::Fingerprint>,
}

/// Phase 2a (cheap): decide whether a screenshot is due this tick, applying
/// the locked/screensaver gate. Draws and stores the next scheduled time
/// whenever a decision (capture or skip) is made, so pacing continues either
/// way.
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

/// Builds a forced-capture plan, bypassing the interval-due gate (Gate 0)
/// but keeping the locked/screensaver gate (Gate 1). Used for an
/// on-demand "capture now" request rather than the normal cadence. Unlike
/// `plan`, this never touches `state.next_screenshot_at_ms` — the schedule
/// for the next *automatic* capture is left undisturbed — and never
/// enqueues a `ScreenshotSkipped` upload when locked, since there's no
/// scheduled slot being consumed. `anchor` is always `None`, so the
/// fingerprint-diff gate in `capture_and_process` always treats the forced
/// frame as changed (same as the very first capture ever).
pub fn plan_forced(
    state: &ScreenshotState,
    hooks: &dyn ScreenshotHooks,
) -> CoreResult<Option<CapturePlan>> {
    if !state.enabled {
        return Ok(None);
    }

    if hooks.is_locked_or_screensaver()? {
        return Ok(None);
    }

    Ok(Some(CapturePlan { anchor: None }))
}

/// Outcome of the heavy capture pipeline.
pub enum CaptureOutcome {
    Failed,
    StaticFrame,
    Uploaded {
        risk: f32,
        kind: UploadKind,
        fingerprint: Option<fingerprint::Fingerprint>,
    },
}

/// Phase 2b (slow): capture -> fingerprint diff -> classify -> redact ->
/// image process. Unit-testable on its own.
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

    // Decoded once and shared by the fingerprint, the classifier, redaction, and the image
    // pipeline below — all of them only need derivatives of the same decoded frame, so
    // decoding it repeatedly would be a wasted full-resolution allocation each time. A decode
    // failure here is unrecoverable (every step downstream needs a decoded image), so it fails
    // the whole capture immediately rather than limping through steps that would all fail on
    // the same bytes anyway.
    let decoded = match image::load_from_memory(&screenshot.bytes) {
        Ok(img) => img,
        Err(err) => {
            tracing::error!(error = %err, "screenshot decode failed");
            return CaptureOutcome::Failed;
        }
    };

    // Gate 2 — screen-change diff vs the last *uploaded* frame. With no prior uploaded
    // fingerprint we always upload the first frame.
    let fp = fingerprint::fingerprint_from_image(&decoded);
    let static_frame = match plan.anchor.as_ref() {
        Some(prev) => !fingerprint::changed(prev, &fp),
        None => false,
    };
    if static_frame {
        return CaptureOutcome::StaticFrame;
    }

    // Classify before the (consuming) image pipeline. A `None` classifier (model
    // failed to load) or a classify error fails safe to risk 0 with no raw scores,
    // logged so an always-0 misconfiguration is diagnosable.
    let scores = classifier.and_then(|c| match c.classify_image(&decoded) {
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

    let redacted = redact_if_ocr(ocr, &screenshot.bytes, decoded);
    let processed = match image_pipeline::ImagePipeline.process_image(redacted, screenshot.captured_at_ms)
    {
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
        fingerprint: Some(fp),
    }
}

/// Phase 2c: apply the capture outcome.
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

/// Detects and blacks out on-screen text, operating on the already-decoded frame (`img`) so
/// this doesn't need its own PNG decode — `engine.detect` still needs the original encoded
/// `bytes` since Vision/Tesseract/etc. decode the frame themselves internally.
fn redact_if_ocr(
    ocr: Option<&ScreenshotOCR>,
    bytes: &[u8],
    img: image::DynamicImage,
) -> image::DynamicImage {
    let Some(engine) = ocr else {
        return img;
    };
    match engine.detect(bytes) {
        Ok(result) if !result.regions.is_empty() => redact_text_regions(img, &result.regions),
        Ok(_) => img,
        Err(err) => {
            tracing::warn!(error = %err, "OCR failed, uploading unredacted");
            img
        }
    }
}

fn redact_text_regions(
    img: image::DynamicImage,
    regions: &[virtue_text_detection::TextRegion],
) -> image::DynamicImage {
    let mut img = img.to_rgba8();
    for r in regions {
        let bb = &r.bounding_box;
        for py in (bb.y as u32)..((bb.y + bb.height) as u32).min(img.height()) {
            for px in (bb.x as u32)..((bb.x + bb.width) as u32).min(img.width()) {
                img.put_pixel(px, py, image::Rgba([0, 0, 0, 255]));
            }
        }
    }
    image::DynamicImage::ImageRgba8(img)
}

/// Builds the (lazily-initializing) NSFW classifier. Construction itself is infallible — the
/// model isn't actually parsed/built until the first classification that needs it; see
/// [`RiskClassifier::new`].
pub fn load_classifier() -> Option<RiskClassifier> {
    #[cfg(not(test))]
    {
        Some(RiskClassifier::new(MODEL_BYTES))
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
    fn plan_forced_bypasses_the_interval_due_gate() {
        let mut state = ScreenshotState::default();
        enable(&mut state);
        state.next_screenshot_at_ms = Some(1_000_000_000);
        let hooks = TestPlatformHooks::new();
        let plan = plan_forced(&state, &hooks).unwrap();
        assert!(plan.is_some());
        assert_eq!(
            state.next_screenshot_at_ms,
            Some(1_000_000_000),
            "plan_forced must not disturb the normal capture schedule"
        );
    }

    #[test]
    fn plan_forced_still_respects_locked_or_screensaver() {
        let mut state = ScreenshotState::default();
        enable(&mut state);
        let hooks = TestPlatformHooks::new();
        hooks.set_locked_or_screensaver(true);
        let plan = plan_forced(&state, &hooks).unwrap();
        assert!(plan.is_none());
    }

    #[test]
    fn plan_forced_is_a_noop_when_disabled() {
        let state = ScreenshotState::default();
        let hooks = TestPlatformHooks::new();
        let plan = plan_forced(&state, &hooks).unwrap();
        assert!(plan.is_none());
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
