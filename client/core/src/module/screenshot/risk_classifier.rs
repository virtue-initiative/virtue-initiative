use image::DynamicImage;
use tract_onnx::prelude::*;

use crate::error::{CoreError, CoreResult};

const INPUT_SIZE: u32 = 224;

// Skin heuristic constants.
const SKIN_DOWNSAMPLE: u32 = 64; // count skin pixels on a 64×64 thumbnail (~1ms)
const SKIN_SATURATION_RATIO: f32 = 0.30; // skin_score saturates at 30% skin pixels
const SKIN_WEIGHT: f32 = 0.30; // skin contributes up to 30% of risk
const MODEL_WEIGHT: f32 = 0.70; // model contributes up to 70% of risk
const SKIN_GATE: f32 = 0.05; // below this contribution (~1.5% skin) skip the model

type NsfwModel = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

/// Raw, unweighted outputs of the two-stage NSFW cascade plus the blended risk.
///
/// `risk` is what drives severity/alerting; `skin` and `nsfw` are the raw stage scores
/// recorded on the log as low-level dev metadata so a reviewer can see *why* a frame scored
/// the way it did.
pub struct RiskScores {
    /// Blended risk ∈ [0.0, 1.0] = `skin * SKIN_WEIGHT + nsfw * MODEL_WEIGHT` (or just the
    /// skin contribution when the gate skips the model).
    pub risk: f32,
    /// Raw skin-tone heuristic score ∈ [0.0, 1.0], before weighting.
    pub skin: f32,
    /// Raw NSFW model probability ∈ [0.0, 1.0]. `None` when the skin gate skipped the model
    /// (i.e. negligible skin), so it's distinguishable from "model ran and returned 0".
    pub nsfw: Option<f32>,
}

pub struct RiskClassifier {
    model: NsfwModel,
}

impl RiskClassifier {
    pub fn new(model_bytes: &[u8]) -> CoreResult<Self> {
        let n = INPUT_SIZE as i32;
        let model = tract_onnx::onnx()
            .model_for_read(&mut std::io::Cursor::new(model_bytes))
            .map_err(|e| CoreError::Classifier(e.to_string()))?
            // MobileNetV2 NSFW model expects a fixed NHWC [1, 224, 224, 3] f32 input.
            .with_input_fact(0, f32::fact([1, n, n, 3]).into())
            .map_err(|e| CoreError::Classifier(e.to_string()))?
            .into_optimized()
            .map_err(|e| CoreError::Classifier(e.to_string()))?
            .into_runnable()
            .map_err(|e| CoreError::Classifier(e.to_string()))?;
        Ok(Self { model })
    }

    /// Returns the [`RiskScores`] for an image using a two-stage cascade:
    ///
    /// 1. A cheap YCbCr skin-tone heuristic (~1ms, no model) contributes up to 30%.
    /// 2. Only when meaningful skin is present do we pay for the MobileNet NSFW model,
    ///    which contributes up to 70%.
    ///
    /// Most screenshots (terminals, code, docs) have negligible skin and return early
    /// without any ONNX inference (`nsfw = None`), keeping the daemon loop fast enough to
    /// avoid `PingGapWhileRunning` false alerts.
    pub fn classify(&self, image_bytes: &[u8]) -> CoreResult<RiskScores> {
        let img = image::load_from_memory(image_bytes)?;

        let skin = skin_score(&img);
        let contribution = skin * SKIN_WEIGHT;
        if contribution <= SKIN_GATE {
            return Ok(RiskScores {
                risk: contribution,
                skin,
                nsfw: None,
            });
        }

        let model_score = self.run_inference(&img)?;
        Ok(RiskScores {
            risk: contribution + model_score * MODEL_WEIGHT,
            skin,
            nsfw: Some(model_score),
        })
    }

    /// Returns P(nsfw) ∈ [0.0, 1.0] from the MobileNetV2 model for the full image.
    fn run_inference(&self, img: &DynamicImage) -> CoreResult<f32> {
        // MobileNet preprocessing: resize straight to 224×224 (no center crop),
        // RGB channel order, scale pixels to [0, 1].
        let resized = img.resize_exact(
            INPUT_SIZE,
            INPUT_SIZE,
            image::imageops::FilterType::Triangle,
        );
        let rgb = resized.to_rgb8();

        let n = INPUT_SIZE as usize;
        let tensor: Tensor =
            tract_ndarray::Array4::<f32>::from_shape_fn((1, n, n, 3), |(_, y, x, c)| {
                rgb.get_pixel(x as u32, y as u32)[c] as f32 / 255.0
            })
            .into();

        let outputs = self
            .model
            .run(tvec![tensor.into()])
            .map_err(|e| CoreError::Classifier(e.to_string()))?;

        // Output shape [1, 5]: softmax over GantMan classes
        // [drawings, hentai, neutral, porn, sexy]. NSFW = hentai + porn + sexy.
        let view = outputs[0]
            .to_array_view::<f32>()
            .map_err(|e| CoreError::Classifier(e.to_string()))?;
        let nsfw = view[[0, 1]] + view[[0, 3]] + view[[0, 4]];
        Ok(nsfw.clamp(0.0, 1.0))
    }
}

/// Cheap skin-tone heuristic. Downsamples to 64×64 (Nearest) and counts pixels whose
/// YCbCr values fall in a typical skin range, normalising the ratio so it saturates at
/// 30% skin pixels: `min(skin_ratio / 0.30, 1.0)`.
fn skin_score(img: &DynamicImage) -> f32 {
    let small = img.resize_exact(
        SKIN_DOWNSAMPLE,
        SKIN_DOWNSAMPLE,
        image::imageops::FilterType::Nearest,
    );
    let rgb = small.to_rgb8();

    let mut skin = 0u32;
    let mut total = 0u32;
    for pixel in rgb.pixels() {
        total += 1;
        if is_skin(pixel[0], pixel[1], pixel[2]) {
            skin += 1;
        }
    }
    if total == 0 {
        return 0.0;
    }

    let ratio = skin as f32 / total as f32;
    (ratio / SKIN_SATURATION_RATIO).min(1.0)
}

/// Returns true if an RGB pixel falls in a typical skin range using BT.601 YCbCr:
/// Y > 80, Cb ∈ [77, 127], Cr ∈ [133, 173].
fn is_skin(r: u8, g: u8, b: u8) -> bool {
    let r = r as f32;
    let g = g as f32;
    let b = b as f32;
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    let cb = 128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b;
    let cr = 128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b;
    y > 80.0 && (77.0..=127.0).contains(&cb) && (133.0..=173.0).contains(&cr)
}

#[cfg(test)]
mod tests {
    use super::skin_score;
    use image::{DynamicImage, RgbImage};

    fn solid(r: u8, g: u8, b: u8) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(128, 128, image::Rgb([r, g, b])))
    }

    #[test]
    fn pure_white_has_zero_skin_score() {
        assert_eq!(skin_score(&solid(255, 255, 255)), 0.0);
    }

    #[test]
    fn pure_black_has_zero_skin_score() {
        assert_eq!(skin_score(&solid(0, 0, 0)), 0.0);
    }

    #[test]
    fn skin_tone_image_scores_above_zero() {
        // A typical light skin tone (240, 200, 160): Y≈207, Cb≈101, Cr≈151 — all in range.
        let score = skin_score(&solid(240, 200, 160));
        assert!(score > 0.0, "skin tone should score > 0, got {score}");
        // A fully skin-toned image saturates the score at 1.0.
        assert_eq!(score, 1.0);
    }
}
