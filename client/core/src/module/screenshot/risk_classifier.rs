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

// Per-region scoring constants (see `classify_regions`). They were tuned on a labeled set of
// 338 screenshots with `scripts/nsfw-eval/`, for no ordinary page at high risk.
pub const MAX_REGIONS: usize = 12;
// A region's own skin gate: the model runs on it only above ~15% skin pixels. Stricter than
// the whole-frame gate because each region is one image, not a page.
const REGION_SKIN_GATE: f32 = 0.15;
// Weight of the "sexy" class (swimwear, lingerie). Those are meant to be flagged.
const REGION_SEXY_WEIGHT: f32 = 1.0;
// A region scoring at least this gets a second look on its mirror image.
const CONFIRM_AT: f32 = 0.5;
// Region scores at or below this floor count as 0, and the rest are rescaled to [0, 1].
// Each extra model run is another chance at a stray mid-range score; raising the bar per
// region keeps those from adding up to an alert, while a confident hit barely moves.
const REGION_SCORE_FLOOR: f32 = 0.7;
// Small regions are upscaled a lot to reach 224×224, which makes the model noisier, so their
// score is scaled from SMALL_REGION_WEIGHT at MIN_SIDE up to 1.0 at FULL_WEIGHT_SIDE.
const SMALL_REGION_WEIGHT: f32 = 0.75;
const FULL_WEIGHT_SIDE: u32 = 160;
// High risk needs MIN_AGREE regions scoring at least AGREE_AT before the floor, or one such
// region covering AGREE_SHARE of its window (desktop) or of the screen (phone). The model's
// confident misfires on ordinary pages are single photos; a porn page shows many at once.
const MIN_AGREE: u32 = 3;
const AGREE_AT: f32 = 0.9;
const AGREE_SHARE: f32 = 0.2;
// Without agreement, risk is held just under the high band (HIGH_RISK_PERCENT in upload.rs).
const BELOW_HIGH: f32 = 0.69;

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

/// What kind of screen a screenshot comes from. Desktop screens can show browser windows,
/// so a lone image is judged against the window it sits in; phones show one page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Desktop,
    Phone,
}

/// How one extracted region scored in [`RiskClassifier::classify_regions`].
pub struct RegionScore {
    pub region: Region,
    /// The window the region was found in (see [`image_regions::Extracted::container`]).
    pub container: Region,
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

/// A built model, for evaluation tooling that needs raw class probabilities for images the
/// gates would skip. Production code goes through [`RiskClassifier`]'s `classify_*` methods,
/// which build and drop the model per call.
pub struct LoadedModel(NsfwModel);

impl LoadedModel {
    /// Class probabilities `[drawings, hentai, neutral, porn, sexy]` for the image.
    pub fn probs(&self, img: &DynamicImage) -> CoreResult<[f32; 5]> {
        RiskClassifier::run_probs(&self.0, img)
    }
}

/// The constants behind [`RiskClassifier::classify_image`] and
/// [`RiskClassifier::classify_regions`], by name. The offline scorer in
/// `scripts/nsfw-eval/` takes its defaults from these so they can't drift apart.
pub fn scoring_constants() -> Vec<(&'static str, f64)> {
    vec![
        ("skin_saturation_ratio", SKIN_SATURATION_RATIO as f64),
        ("skin_weight", SKIN_WEIGHT as f64),
        ("model_weight", MODEL_WEIGHT as f64),
        ("skin_gate", SKIN_GATE as f64),
        ("region_skin_gate", REGION_SKIN_GATE as f64),
        ("max_regions", MAX_REGIONS as f64),
        ("region_sexy_weight", REGION_SEXY_WEIGHT as f64),
        ("confirm_at", CONFIRM_AT as f64),
        ("region_score_floor", REGION_SCORE_FLOOR as f64),
        ("small_region_weight", SMALL_REGION_WEIGHT as f64),
        ("full_weight_side", FULL_WEIGHT_SIDE as f64),
        ("min_side", image_regions::MIN_SIDE as f64),
        ("min_agree", MIN_AGREE as f64),
        ("agree_at", AGREE_AT as f64),
        ("agree_share", AGREE_SHARE as f64),
    ]
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
    /// fixes that, but running the model up to [`MAX_REGIONS`] times also gives it more
    /// chances to be wrong, so the per-region scores are tempered before they're combined:
    ///
    /// - Each crop has its own skin gate ([`REGION_SKIN_GATE`]), so most regions (landscapes,
    ///   products, UI art) never reach the model. Grayscale crops skip the gate, since a
    ///   skin-tone test can't see skin in a black-and-white image.
    /// - A crop scoring at least [`CONFIRM_AT`] is run again mirrored, and keeps the lower of
    ///   the two scores. A real explicit image scores high both ways; a fluke often doesn't.
    /// - Scores at or below [`REGION_SCORE_FLOOR`] count as 0 and the rest are rescaled, so
    ///   middling scores can't stand in for a confident one.
    /// - Small regions are scaled down (see [`SMALL_REGION_WEIGHT`]), since upscaling makes
    ///   the model noisier.
    ///
    /// See [`combine`] for how the region scores become the risk. When no region is found
    /// (text, code, a desktop) the whole frame is scored once under the same gate and
    /// grayscale rules. The model is built at most once per call, and only if needed.
    pub fn classify_regions(
        &self,
        img: &DynamicImage,
        screen: Screen,
    ) -> CoreResult<RegionRiskScores> {
        let extracted = image_regions::extract_regions_tagged(
            img,
            MAX_REGIONS,
            &image_regions::ExtractParams::default(),
        );
        let skin = skin_score(img);
        let gray = is_grayscale(img);
        let mut model: Option<NsfwModel> = None;
        let build = |model: &mut Option<NsfwModel>| -> bool {
            if model.is_none() {
                match Self::build_model(self.model_bytes) {
                    Ok(built) => *model = Some(built),
                    Err(err) => {
                        tracing::error!(error = %err, "NSFW classifier unavailable, falling back to skin-only risk");
                        return false;
                    }
                }
            }
            true
        };

        if extracted.is_empty() {
            let gated = skin * SKIN_WEIGHT <= REGION_SKIN_GATE && !gray;
            if gated || !build(&mut model) {
                return Ok(RegionRiskScores {
                    scores: RiskScores {
                        risk: skin * SKIN_WEIGHT,
                        skin,
                        nsfw: None,
                    },
                    regions: Vec::new(),
                    model_runs: 0,
                });
            }
            NSFW_MODEL_INVOCATIONS.fetch_add(1, Ordering::Relaxed);
            let nsfw = Self::run_inference(model.as_ref().expect("built above"), img)?;
            let skin_term = if gray { nsfw } else { skin };
            return Ok(RegionRiskScores {
                scores: RiskScores {
                    risk: skin_term * SKIN_WEIGHT + nsfw * MODEL_WEIGHT,
                    skin,
                    nsfw: Some(nsfw),
                },
                regions: Vec::new(),
                model_runs: 1,
            });
        }

        let screen_area = img.width() as u64 * img.height() as u64;
        let mut model_runs = 0;
        let mut results = Vec::with_capacity(extracted.len());
        let mut scored = Vec::new();
        for e in extracted {
            let region = e.region;
            let crop = img.crop_imm(region.x, region.y, region.width, region.height);
            let crop_skin = skin_score(&crop);
            let crop_gray = is_grayscale(&crop);
            let mut result = RegionScore {
                region,
                container: e.container,
                skin: crop_skin,
                probs: None,
                confirm_probs: None,
                score: None,
            };
            if (crop_skin * SKIN_WEIGHT > REGION_SKIN_GATE || crop_gray) && build(&mut model) {
                let model = model.as_ref().expect("built above");
                let probs = Self::run_probs(model, &crop)?;
                model_runs += 1;
                let mut raw = region_nsfw(&probs);
                if raw >= CONFIRM_AT {
                    let mirrored = Self::run_probs(model, &crop.fliph())?;
                    model_runs += 1;
                    raw = raw.min(region_nsfw(&mirrored));
                    result.confirm_probs = Some(mirrored);
                }
                result.probs = Some(probs);
                let above_floor =
                    ((raw - REGION_SCORE_FLOOR) / (1.0 - REGION_SCORE_FLOOR)).max(0.0);
                let score = above_floor * size_weight(&region);
                result.score = Some(score);
                let frame = match screen {
                    Screen::Desktop => e.container.area(),
                    Screen::Phone => screen_area,
                };
                scored.push(ScoredRegion {
                    score,
                    raw,
                    skin: if crop_gray { score } else { crop_skin },
                    prominent: region.area() as f32 >= AGREE_SHARE * frame as f32,
                });
            }
            results.push(result);
        }
        NSFW_MODEL_INVOCATIONS.fetch_add(model_runs as u64, Ordering::Relaxed);

        let (risk, nsfw) = combine(skin, gray, &scored);
        Ok(RegionRiskScores {
            scores: RiskScores { risk, skin, nsfw },
            regions: results,
            model_runs,
        })
    }

    /// Builds the model for repeated use; see [`LoadedModel`].
    pub fn load_model(&self) -> CoreResult<LoadedModel> {
        Self::build_model(self.model_bytes).map(LoadedModel)
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

/// A region the model ran on, as [`combine`] sees it.
struct ScoredRegion {
    /// After the floor and size weighting.
    score: f32,
    /// Before the floor: the class sum, or the lower of it and the mirrored pass.
    raw: f32,
    /// The region's skin score, or for a grayscale region (whose skin can't be measured)
    /// its own `score`.
    skin: f32,
    /// Covers at least [`AGREE_SHARE`] of its window (desktop) or of the screen (phone).
    prominent: bool,
}

/// Combines region scores into `(risk, nsfw)`. `nsfw` is the best region score. The skin
/// term comes from that best region, since the whole frame's skin is diluted by everything
/// else on screen; with no region scored it's the whole frame's skin, or 0 for a grayscale
/// frame. High risk needs [`MIN_AGREE`] regions at [`AGREE_AT`] or one prominent one;
/// otherwise the risk is held at [`BELOW_HIGH`].
fn combine(frame_skin: f32, frame_gray: bool, scored: &[ScoredRegion]) -> (f32, Option<f32>) {
    // On a tie the first (largest) region wins, as in score.py.
    let best = scored
        .iter()
        .reduce(|best, r| if r.score > best.score { r } else { best });
    let nsfw = best.map(|b| b.score);
    let skin = match best {
        Some(b) => b.skin,
        None if frame_gray => 0.0,
        None => frame_skin,
    };
    let mut risk = skin * SKIN_WEIGHT + nsfw.unwrap_or(0.0) * MODEL_WEIGHT;
    let agreeing = scored.iter().filter(|r| r.raw >= AGREE_AT);
    let agree_count = agreeing.clone().count() as u32;
    let prominent = agreeing.clone().any(|r| r.prominent);
    if agree_count < MIN_AGREE && !prominent {
        risk = risk.min(BELOW_HIGH);
    }
    (risk, nsfw)
}

/// A region's NSFW score from its class probabilities, with "sexy" weighted.
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
pub fn is_grayscale(img: &DynamicImage) -> bool {
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
pub fn skin_score(img: &DynamicImage) -> f32 {
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
    fn region_score_counts_all_three_nsfw_classes() {
        // [drawings, hentai, neutral, porn, sexy]
        assert_eq!(region_nsfw(&[0.0, 0.0, 0.2, 0.0, 0.8]), 0.8);
        assert_eq!(region_nsfw(&[0.0, 0.1, 0.0, 0.9, 0.0]), 1.0);
        assert_eq!(region_nsfw(&[0.9, 0.0, 0.1, 0.0, 0.0]), 0.0);
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

    fn scored(score: f32, raw: f32, skin: f32, prominent: bool) -> ScoredRegion {
        ScoredRegion {
            score,
            raw,
            skin,
            prominent,
        }
    }

    #[test]
    fn one_confident_image_is_held_under_high() {
        let (risk, nsfw) = combine(0.1, false, &[scored(1.0, 1.0, 1.0, false)]);
        assert_eq!(nsfw, Some(1.0));
        assert_eq!(risk, BELOW_HIGH);
    }

    #[test]
    fn three_agreeing_images_reach_high() {
        let three = [
            scored(1.0, 1.0, 1.0, false),
            scored(0.9, 0.95, 1.0, false),
            scored(0.8, 0.91, 0.5, false),
        ];
        let (risk, _) = combine(0.1, false, &three);
        assert!((risk - 1.0).abs() < 1e-6, "{risk}");
    }

    #[test]
    fn one_prominent_image_reaches_high_only_if_confident() {
        let (risk, _) = combine(0.1, false, &[scored(1.0, 1.0, 1.0, true)]);
        assert!((risk - 1.0).abs() < 1e-6, "{risk}");
        // Prominent, but below the agreement bar.
        let (risk, _) = combine(0.1, false, &[scored(0.5, 0.85, 1.0, true)]);
        assert!(risk <= BELOW_HIGH, "{risk}");
    }

    #[test]
    fn skin_comes_from_the_best_region_and_the_first_on_a_tie() {
        let regions = [scored(1.0, 1.0, 0.2, true), scored(1.0, 1.0, 1.0, true)];
        let (risk, _) = combine(1.0, false, &regions);
        assert!(
            (risk - (0.2 * SKIN_WEIGHT + MODEL_WEIGHT)).abs() < 1e-6,
            "{risk}"
        );
    }

    #[test]
    fn grayscale_frames_without_scored_regions_add_no_skin() {
        assert_eq!(combine(0.8, true, &[]), (0.0, None));
        let (risk, nsfw) = combine(0.8, false, &[]);
        assert_eq!(nsfw, None);
        assert!((risk - 0.8 * SKIN_WEIGHT).abs() < 1e-6);
    }
}
