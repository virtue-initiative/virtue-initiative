use std::sync::atomic::{AtomicU64, Ordering};

use image::DynamicImage;
use tract_nnef::prelude::*;

use super::image_regions::{self, Region};
use crate::error::{CoreError, CoreResult};

static NSFW_MODEL_INVOCATIONS: AtomicU64 = AtomicU64::new(0);

/// Process-lifetime count of `classify()` calls that actually ran the NSFW
/// model (i.e. the skin gate didn't skip it) — not persisted across
/// restarts, just a lightweight always-available tally for platform-side
/// memory diagnostics (see the iOS `virtue_ios_native_nsfw_run_count` FFI
/// export).
pub fn nsfw_model_invocation_count() -> u64 {
    NSFW_MODEL_INVOCATIONS.load(Ordering::Relaxed)
}

const INPUT_SIZE: u32 = 224;

// Skin heuristic constants.
const SKIN_DOWNSAMPLE: u32 = 64; // count skin pixels on a 64×64 thumbnail (~1ms)
const SKIN_SATURATION_RATIO: f32 = 0.30; // skin_score saturates at 30% skin pixels
const SKIN_WEIGHT: f32 = 0.30; // skin contributes up to 30% of risk
const MODEL_WEIGHT: f32 = 0.70; // model contributes up to 70% of risk
const SKIN_GATE: f32 = 0.05; // below this contribution (~1.5% skin) skip the model
const GRAY_CHROMA: u8 = 24; // a pixel whose channels differ by more than this has color
const GRAY_MAX_COLORED: f32 = 0.05; // an image with fewer colored pixels than this is grayscale

// Per-region scoring constants (see `classify_regions`).
pub const MAX_REGIONS: usize = 6;
// "sexy" (swimwear, lingerie ads, fitness) is the class that fires most on ordinary feeds
// and storefronts, and a screenshot full of thumbnails gives it many more chances to.
const REGION_SEXY_WEIGHT: f32 = 0.5;
// A region scoring at least this gets a second look on its mirror image.
const CONFIRM_AT: f32 = 0.5;
// Region scores at or below this floor count as 0, and the rest are rescaled to [0, 1].
// Each extra model run is another chance at a stray mid-range score; raising the bar per
// region keeps those from adding up to an alert, while a confident hit barely moves.
const REGION_SCORE_FLOOR: f32 = 0.3;
// Small regions are upscaled a lot to reach 224×224, which makes the model noisier, so their
// score is scaled from SMALL_REGION_WEIGHT at MIN_SIDE up to 1.0 at FULL_WEIGHT_SIDE.
const SMALL_REGION_WEIGHT: f32 = 0.75;
const FULL_WEIGHT_SIDE: u32 = 160;

// Model output order: GantMan classes [drawings, hentai, neutral, porn, sexy].
const HENTAI: usize = 1;
const PORN: usize = 3;
const SEXY: usize = 4;

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

/// How one extracted region scored in [`RiskClassifier::classify_regions`].
pub struct RegionScore {
    pub region: Region,
    /// Raw skin-tone score of the crop.
    pub skin: f32,
    /// Model class probabilities `[drawings, hentai, neutral, porn, sexy]`, or `None` when the
    /// crop's skin gate skipped the model.
    pub probs: Option<[f32; 5]>,
    /// Probabilities for the mirrored crop, when the first pass reached [`CONFIRM_AT`].
    pub confirm_probs: Option<[f32; 5]>,
    /// The region's final score after reweighting, confirmation, the floor and size weighting.
    pub score: Option<f32>,
}

/// Result of [`RiskClassifier::classify_regions`].
pub struct RegionRiskScores {
    /// Same shape as the whole-frame result; `nsfw` is the aggregated region score.
    pub scores: RiskScores,
    /// Every extracted region, largest first. Empty means the whole frame was classified.
    pub regions: Vec<RegionScore>,
    /// How many times the model ran, including confirmation passes.
    pub model_runs: u32,
}

pub struct RiskClassifier {
    model_bytes: &'static [u8],
}

impl RiskClassifier {
    /// `model_bytes` is an NNEF tar (not ONNX) — pre-converted offline from the source ONNX
    /// model (see `models/README.md` for how to regenerate it). The conversion deliberately
    /// stops at `into_typed()` rather than `into_optimized()`: the optimizer fuses ops (e.g.
    /// `OptMulByScalar`) that NNEF's serializer has no writer for, so optimization happens
    /// here at load time instead, on the NNEF-loaded model. Building the model (NNEF parse + optimize) is
    /// deferred to the first [`classify`](Self::classify_image) call that actually needs it, and
    /// the built model is dropped again as soon as that call returns — measured at ~40MB on iOS
    /// — rather than being cached for the process's lifetime. The cheap skin-tone gate below
    /// skips the model entirely for most screenshots (terminals, code, docs), so caching would
    /// buy nothing in the common case where the extension process only ever handles one capture
    /// (CORE-015); the one case it *would* help — a process that survives across several
    /// captures — is exactly the case where caching is harmful, since it would turn a transient
    /// ~40MB cost into a permanent baseline for the rest of that process's life, stacking with
    /// every later capture's own decode/redact allocations. Rebuilding costs a few seconds of
    /// CPU on each triggering capture instead, which is preferable to that standing memory risk.
    pub fn new(model_bytes: &'static [u8]) -> Self {
        Self { model_bytes }
    }

    /// Returns the [`RiskScores`] for an image using a two-stage cascade:
    ///
    /// 1. A cheap YCbCr skin-tone heuristic (~1ms, no model) contributes up to 30%.
    /// 2. Only when meaningful skin is present do we pay for the MobileNet NSFW model,
    ///    which contributes up to 70%.
    ///
    /// Most screenshots (terminals, code, docs) have negligible skin and return early
    /// without ever building the model (`nsfw = None`), keeping the daemon loop fast enough to
    /// avoid `UnexpectedGap` false alerts.
    pub fn classify(&self, image_bytes: &[u8]) -> CoreResult<RiskScores> {
        let img = image::load_from_memory(image_bytes)?;
        self.classify_image(&img)
    }

    /// Same as [`classify`](Self::classify), but takes an already-decoded image so a caller
    /// that also needs the decoded image for something else (e.g. the fingerprint) doesn't pay
    /// for a second full PNG decode of the same bytes.
    pub fn classify_image(&self, img: &DynamicImage) -> CoreResult<RiskScores> {
        let skin = skin_score(img);
        let contribution = skin * SKIN_WEIGHT;
        if contribution <= SKIN_GATE {
            return Ok(RiskScores {
                risk: contribution,
                skin,
                nsfw: None,
            });
        }

        let model = match Self::build_model(self.model_bytes) {
            Ok(model) => model,
            Err(err) => {
                // Model unavailable (failed to build) — fail safe to the skin-only
                // contribution, same as a skipped gate, but log loudly since this means every
                // capture's risk score is missing its model component until fixed.
                tracing::error!(error = %err, "NSFW classifier unavailable, falling back to skin-only risk");
                return Ok(RiskScores {
                    risk: contribution,
                    skin,
                    nsfw: None,
                });
            }
        };

        NSFW_MODEL_INVOCATIONS.fetch_add(1, Ordering::Relaxed);
        let model_score = Self::run_inference(&model, img)?;
        Ok(RiskScores {
            risk: contribution + model_score * MODEL_WEIGHT,
            skin,
            nsfw: Some(model_score),
        })
    }

    /// Scores the photo-like regions of a screenshot one at a time instead of the whole frame.
    ///
    /// Classifying the whole screen squashes a page of text and a few images into 224×224,
    /// where an explicit thumbnail is a handful of pixels. Classifying each extracted image
    /// fixes that, but running the model up to six times also gives it six chances to be
    /// wrong, so the per-region scores are tempered before they're combined:
    ///
    /// - Each crop has its own skin gate, so most regions (landscapes, products, UI art) never
    ///   reach the model. Grayscale crops skip the gate, since a skin-tone test can't see skin
    ///   in a black-and-white photo.
    /// - The "sexy" class counts at [`REGION_SEXY_WEIGHT`], since it's the one that fires on
    ///   ordinary ads and fashion photos.
    /// - A crop scoring at least [`CONFIRM_AT`] is run again mirrored, and keeps the lower of
    ///   the two scores. A real explicit image scores high both ways; a fluke often doesn't.
    /// - Scores at or below [`REGION_SCORE_FLOOR`] count as 0 and the rest are rescaled, so
    ///   six middling scores can't stand in for one confident one.
    /// - Small regions are scaled down (see [`SMALL_REGION_WEIGHT`]), since upscaling makes
    ///   the model noisier.
    ///
    /// The highest region score becomes `nsfw`. Agreement between regions isn't counted as
    /// extra evidence: thumbnails on one page share a topic, so they tend to be wrong together.
    /// The skin term still comes from the whole frame, which already weights each image's skin
    /// by how much of the screen it fills.
    ///
    /// When no region is found (text, code, a desktop) it falls back to [`classify_image`](Self::classify_image).
    /// The model is built at most once per call, and only if some crop passes its gate.
    pub fn classify_regions(&self, img: &DynamicImage) -> CoreResult<RegionRiskScores> {
        let regions = image_regions::extract_regions(img, MAX_REGIONS);
        if regions.is_empty() {
            let scores = self.classify_image(img)?;
            let model_runs = scores.nsfw.is_some() as u32;
            return Ok(RegionRiskScores {
                scores,
                regions: Vec::new(),
                model_runs,
            });
        }

        let skin = skin_score(img);
        let mut model: Option<NsfwModel> = None;
        let mut model_runs = 0;
        let mut results = Vec::with_capacity(regions.len());
        for region in regions {
            let crop = img.crop_imm(region.x, region.y, region.width, region.height);
            let crop_skin = skin_score(&crop);
            let mut result = RegionScore {
                region,
                skin: crop_skin,
                probs: None,
                confirm_probs: None,
                score: None,
            };
            if crop_skin * SKIN_WEIGHT > SKIN_GATE || is_grayscale(&crop) {
                if model.is_none() {
                    match Self::build_model(self.model_bytes) {
                        Ok(built) => model = Some(built),
                        Err(err) => {
                            tracing::error!(error = %err, "NSFW classifier unavailable, falling back to skin-only risk");
                            results.push(result);
                            break;
                        }
                    }
                }
                let model = model.as_ref().expect("built above");
                let probs = Self::run_probs(model, &crop)?;
                model_runs += 1;
                let mut score = region_nsfw(&probs);
                if score >= CONFIRM_AT {
                    let mirrored = Self::run_probs(model, &crop.fliph())?;
                    model_runs += 1;
                    score = score.min(region_nsfw(&mirrored));
                    result.confirm_probs = Some(mirrored);
                }
                result.probs = Some(probs);
                let above_floor =
                    ((score - REGION_SCORE_FLOOR) / (1.0 - REGION_SCORE_FLOOR)).max(0.0);
                result.score = Some(above_floor * size_weight(&region));
            }
            results.push(result);
        }
        NSFW_MODEL_INVOCATIONS.fetch_add(model_runs as u64, Ordering::Relaxed);

        let nsfw = results.iter().filter_map(|r| r.score).reduce(f32::max);
        let risk = skin * SKIN_WEIGHT + nsfw.unwrap_or(0.0) * MODEL_WEIGHT;
        Ok(RegionRiskScores {
            scores: RiskScores { risk, skin, nsfw },
            regions: results,
            model_runs,
        })
    }

    fn build_model(model_bytes: &[u8]) -> CoreResult<NsfwModel> {
        tract_nnef::nnef()
            .model_for_read(&mut std::io::Cursor::new(model_bytes))
            .map_err(|e| CoreError::Classifier(e.to_string()))?
            .into_optimized()
            .map_err(|e| CoreError::Classifier(e.to_string()))?
            .into_runnable()
            .map_err(|e| CoreError::Classifier(e.to_string()))
    }

    /// Returns P(nsfw) ∈ [0.0, 1.0] from the MobileNetV2 model for the full image.
    fn run_inference(model: &NsfwModel, img: &DynamicImage) -> CoreResult<f32> {
        let probs = Self::run_probs(model, img)?;
        // NSFW = hentai + porn + sexy.
        Ok((probs[HENTAI] + probs[PORN] + probs[SEXY]).clamp(0.0, 1.0))
    }

    /// Returns the model's five class probabilities for the image.
    fn run_probs(model: &NsfwModel, img: &DynamicImage) -> CoreResult<[f32; 5]> {
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

        let outputs = model
            .run(tvec![tensor.into()])
            .map_err(|e| CoreError::Classifier(e.to_string()))?;

        // Output shape [1, 5]: softmax over GantMan classes
        // [drawings, hentai, neutral, porn, sexy].
        let view = outputs[0]
            .to_array_view::<f32>()
            .map_err(|e| CoreError::Classifier(e.to_string()))?;
        Ok(std::array::from_fn(|i| view[[0, i]]))
    }
}

/// A region's NSFW score from its class probabilities, with "sexy" down-weighted.
fn region_nsfw(probs: &[f32; 5]) -> f32 {
    (probs[HENTAI] + probs[PORN] + probs[SEXY] * REGION_SEXY_WEIGHT).clamp(0.0, 1.0)
}

/// Down-weights regions that need a lot of upscaling to reach the model's input size.
fn size_weight(region: &Region) -> f32 {
    let side = region.width.min(region.height);
    let t = (side.saturating_sub(image_regions::MIN_SIDE) as f32
        / (FULL_WEIGHT_SIDE - image_regions::MIN_SIDE) as f32)
        .min(1.0);
    SMALL_REGION_WEIGHT + (1.0 - SMALL_REGION_WEIGHT) * t
}

/// True if almost no pixel of a 64×64 thumbnail has visible color: a black-and-white photo.
fn is_grayscale(img: &DynamicImage) -> bool {
    let small = img.resize_exact(
        SKIN_DOWNSAMPLE,
        SKIN_DOWNSAMPLE,
        image::imageops::FilterType::Nearest,
    );
    let rgb = small.to_rgb8();
    let colored = rgb
        .pixels()
        .filter(|p| p.0.iter().max().unwrap() - p.0.iter().min().unwrap() > GRAY_CHROMA)
        .count();
    (colored as f32) < GRAY_MAX_COLORED * rgb.pixels().len() as f32
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
    use super::*;
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

    #[test]
    fn grayscale_is_detected_and_color_is_not() {
        let gray = DynamicImage::ImageRgb8(RgbImage::from_fn(128, 128, |x, y| {
            let v = ((x + y) % 256) as u8;
            image::Rgb([v, v, v])
        }));
        assert!(is_grayscale(&gray));
        assert!(!is_grayscale(&solid(240, 200, 160)));
    }

    #[test]
    fn region_score_counts_sexy_at_reduced_weight() {
        // [drawings, hentai, neutral, porn, sexy]
        assert_eq!(region_nsfw(&[0.0, 0.0, 0.2, 0.0, 0.8]), 0.4);
        assert_eq!(region_nsfw(&[0.0, 0.1, 0.0, 0.9, 0.0]), 1.0);
    }

    #[test]
    fn small_regions_are_down_weighted() {
        let region = |side| Region {
            x: 0,
            y: 0,
            width: side,
            height: 400,
        };
        assert_eq!(
            size_weight(&region(image_regions::MIN_SIDE)),
            SMALL_REGION_WEIGHT
        );
        assert_eq!(size_weight(&region(FULL_WEIGHT_SIDE)), 1.0);
        assert_eq!(size_weight(&region(1000)), 1.0);
    }
}
