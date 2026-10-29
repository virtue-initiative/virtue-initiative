use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};

use image::DynamicImage;
use tract_nnef::prelude::*;

use super::image_regions::{self, Region};
use crate::error::{CoreError, CoreResult};
use crate::model::ScoredImage;

static NSFW_MODEL_INVOCATIONS: AtomicU64 = AtomicU64::new(0);

/// Process-lifetime count of classify calls (one per screenshot, whichever of
/// `classify_image` or `classify_regions`) that actually built and ran the NSFW
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
// The YCbCr skin range needs Y > 80, which leaves out dark skin. With `dark_skin`, a darker
// pixel down to this brightness also counts when it's warm (R > G > B) with a skin-like share of
// red and green; below it, skin can't be told from black. Only `classify_regions` uses it, so
// `classify_image` scores exactly as before while per-image scoring is behind a feature flag.
// Warm dark UI colors (Material brown, Gruvbox's warm grays) pass too: they sit next to the
// darkest Monk tones, with as much or more color, so no per-pixel rule tells them apart. That
// only costs a model run, since skin adds nothing to `classify_regions`' risk.
const DARK_SKIN_MIN_Y: f32 = 30.0;

// Per-region scoring constants (see `classify_regions`). They were tuned on a labeled set of
// 338 screenshots with `scripts/nsfw-eval/`, for no ordinary page at high risk.
pub const MAX_REGIONS: usize = 12;
// A region's own skin gate, in the same units as SKIN_GATE: the model runs on it only above
// ~15% skin pixels. Stricter than the whole-frame gate because each region is one image, not
// a page. It only saves model runs: risk comes from the model alone.
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
    /// Risk ∈ [0.0, 1.0]. From [`RiskClassifier::classify_image`] it's the blend
    /// `skin * SKIN_WEIGHT + nsfw * MODEL_WEIGHT` (or just the skin contribution when the gate
    /// skips the model); from [`RiskClassifier::classify_regions`] it's the model score alone.
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
    /// Whether the crop is grayscale, which lets it skip the skin gate.
    pub gray: bool,
    /// Model class probabilities `[drawings, hentai, neutral, porn, sexy]`, or `None` when the
    /// crop's skin gate skipped the model.
    pub probs: Option<[f32; 5]>,
    /// Probabilities for the mirrored crop, when the first pass reached [`CONFIRM_AT`].
    pub confirm_probs: Option<[f32; 5]>,
    /// The region's final score after reweighting, confirmation, the floor and size weighting.
    pub score: Option<f32>,
}

impl RegionScore {
    /// How the region is reported on its screenshot event (BATCH-003).
    pub fn to_scored_image(&self) -> ScoredImage {
        ScoredImage {
            x: self.region.x,
            y: self.region.y,
            width: self.region.width,
            height: self.region.height,
            skin: self.skin,
            gray: self.gray,
            nsfw: self.probs.as_ref().map(region_nsfw),
            mirrored: self.confirm_probs.as_ref().map(region_nsfw),
            score: self.score,
        }
    }
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
        ("dark_skin_min_y", DARK_SKIN_MIN_Y as f64),
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
        let skin = skin_score(img, false);
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
    ///   skin-tone test can't see skin in a black-and-white image (CORE-004).
    /// - A crop scoring at least [`CONFIRM_AT`] is run again mirrored, and keeps the lower of
    ///   the two scores. A real explicit image scores high both ways; a fluke often doesn't.
    /// - Scores at or below [`REGION_SCORE_FLOOR`] count as 0 and the rest are rescaled, so
    ///   middling scores can't stand in for a confident one.
    /// - Small regions are scaled down (see [`SMALL_REGION_WEIGHT`]), since upscaling makes
    ///   the model noisier.
    ///
    /// See [`combine`] for how the region scores become the risk. Unlike
    /// [`classify_image`](Self::classify_image), skin only decides which crops reach the
    /// model and adds nothing to the risk: once the floor and agreement rules were in, a skin
    /// term changed no high-risk result on the labeled set. When no region is found (text,
    /// code, a desktop) the whole frame is scored the same way, as one image filling its
    /// window, but isn't listed in `regions`. The model is built at most once per call, and
    /// only if needed; if that fails, it isn't tried again for the call's other regions, and
    /// the risk falls back to the skin contribution, as in `classify_image`.
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
        let (skin, gray) = skin_and_gray(img, true);
        let mut model = LazyModel::NotBuilt;

        // With no image found, the whole frame stands in as the only one, unreported.
        let frame = Region {
            x: 0,
            y: 0,
            width: img.width(),
            height: img.height(),
        };
        let reported = !extracted.is_empty();
        let crops: Vec<(Region, Region)> = if reported {
            extracted
                .into_iter()
                .map(|e| (e.region, e.container))
                .collect()
        } else {
            vec![(frame, frame)]
        };

        let screen_area = frame.area();
        let mut model_runs = 0;
        let mut wanted_model = false;
        let mut results = Vec::with_capacity(crops.len());
        let mut scored = Vec::new();
        for (region, container) in crops {
            let crop = if reported {
                Cow::Owned(img.crop_imm(region.x, region.y, region.width, region.height))
            } else {
                Cow::Borrowed(img)
            };
            let (crop_skin, crop_gray) = if reported {
                skin_and_gray(&crop, true)
            } else {
                (skin, gray)
            };
            let mut result = RegionScore {
                region,
                container,
                skin: crop_skin,
                gray: crop_gray,
                probs: None,
                confirm_probs: None,
                score: None,
            };
            // CORE-004: grayscale crops skip the skin gate.
            let gated = crop_skin * SKIN_WEIGHT <= REGION_SKIN_GATE && !crop_gray;
            wanted_model |= !gated;
            if let Some(model) = (!gated).then(|| model.get(self.model_bytes)).flatten() {
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
                    Screen::Desktop => container.area(),
                    Screen::Phone => screen_area,
                };
                scored.push(ScoredRegion {
                    score,
                    raw,
                    prominent: region.area() as f32 >= AGREE_SHARE * frame as f32,
                });
            }
            if reported {
                results.push(result);
            }
        }

        let (risk, nsfw) = if wanted_model && matches!(model, LazyModel::Failed) {
            (skin * SKIN_WEIGHT, None)
        } else {
            combine(&scored)
        };
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

/// The model for one `classify_regions` call: built on first use, and not retried once a
/// build has failed, so a broken model costs one parse and one error per capture.
enum LazyModel {
    NotBuilt,
    Built(Box<NsfwModel>),
    Failed,
}

impl LazyModel {
    fn get(&mut self, model_bytes: &[u8]) -> Option<&NsfwModel> {
        if matches!(self, LazyModel::NotBuilt) {
            *self = match RiskClassifier::build_model(model_bytes) {
                Ok(model) => {
                    NSFW_MODEL_INVOCATIONS.fetch_add(1, Ordering::Relaxed);
                    LazyModel::Built(Box::new(model))
                }
                Err(err) => {
                    tracing::error!(error = %err, "NSFW classifier unavailable, falling back to skin-only risk");
                    LazyModel::Failed
                }
            };
        }
        match self {
            LazyModel::Built(model) => Some(model),
            _ => None,
        }
    }
}

/// A region the model ran on, as [`combine`] sees it.
struct ScoredRegion {
    /// After the floor and size weighting.
    score: f32,
    /// Before the floor: the class sum, or the lower of it and the mirrored pass.
    raw: f32,
    /// Covers at least [`AGREE_SHARE`] of its window (desktop) or of the screen (phone).
    prominent: bool,
}

/// Combines region scores into `(risk, nsfw)`. `nsfw` is the best region score, and the risk
/// is that score too, or 0 with no region scored. High risk needs [`MIN_AGREE`] regions at
/// [`AGREE_AT`] or one prominent one; otherwise the risk is held at [`BELOW_HIGH`].
fn combine(scored: &[ScoredRegion]) -> (f32, Option<f32>) {
    let nsfw = scored.iter().map(|r| r.score).reduce(f32::max);
    let mut risk = nsfw.unwrap_or(0.0);
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
    thumbnail_is_gray(&thumbnail(img))
}

/// Cheap skin-tone heuristic. Downsamples to 64×64 (Nearest) and counts pixels whose
/// YCbCr values fall in a typical skin range, normalising the ratio so it saturates at
/// 30% skin pixels: `min(skin_ratio / 0.30, 1.0)`. `dark_skin` also counts darker skin
/// pixels (see [`is_skin`]).
pub fn skin_score(img: &DynamicImage, dark_skin: bool) -> f32 {
    thumbnail_skin(&thumbnail(img), dark_skin)
}

/// [`skin_score`] and [`is_grayscale`] from one shared thumbnail.
fn skin_and_gray(img: &DynamicImage, dark_skin: bool) -> (f32, bool) {
    let small = thumbnail(img);
    (thumbnail_skin(&small, dark_skin), thumbnail_is_gray(&small))
}

fn thumbnail(img: &DynamicImage) -> image::RgbImage {
    img.resize_exact(
        SKIN_DOWNSAMPLE,
        SKIN_DOWNSAMPLE,
        image::imageops::FilterType::Nearest,
    )
    .to_rgb8()
}

fn thumbnail_is_gray(rgb: &image::RgbImage) -> bool {
    let colored = rgb
        .pixels()
        .filter(|p| p.0.iter().max().unwrap() - p.0.iter().min().unwrap() > GRAY_CHROMA)
        .count();
    (colored as f32) < GRAY_MAX_COLORED * rgb.pixels().len() as f32
}

fn thumbnail_skin(rgb: &image::RgbImage, dark_skin: bool) -> f32 {
    let mut skin = 0u32;
    let mut total = 0u32;
    for pixel in rgb.pixels() {
        total += 1;
        if is_skin(pixel[0], pixel[1], pixel[2], dark_skin) {
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
/// Y > 80, Cb ∈ [77, 127], Cr ∈ [133, 173]. With `dark_skin`, darker pixels, down to
/// [`DARK_SKIN_MIN_Y`], also count when R > G > B with a normalized red share in [0.36, 0.465] and green share in
/// [0.28, 0.363] (Soriano et al.'s skin locus), which covers the darkest Monk skin tones.
fn is_skin(r: u8, g: u8, b: u8, dark_skin: bool) -> bool {
    let r = r as f32;
    let g = g as f32;
    let b = b as f32;
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    if y > 80.0 {
        let cb = 128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b;
        let cr = 128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b;
        return (77.0..=127.0).contains(&cb) && (133.0..=173.0).contains(&cr);
    }
    if !dark_skin || y < DARK_SKIN_MIN_Y || !(r > g && g > b) {
        return false;
    }
    let sum = r + g + b;
    (0.36..=0.465).contains(&(r / sum)) && (0.28..=0.363).contains(&(g / sum))
}

// CORE-004: the skin tone score.
#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbImage};

    fn solid(r: u8, g: u8, b: u8) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(128, 128, image::Rgb([r, g, b])))
    }

    #[test]
    fn pure_white_has_zero_skin_score() {
        assert_eq!(skin_score(&solid(255, 255, 255), true), 0.0);
    }

    #[test]
    fn pure_black_has_zero_skin_score() {
        assert_eq!(skin_score(&solid(0, 0, 0), true), 0.0);
    }

    #[test]
    fn skin_tone_image_scores_above_zero() {
        // A typical light skin tone (240, 200, 160): Y≈207, Cb≈101, Cr≈151 — all in range.
        let score = skin_score(&solid(240, 200, 160), false);
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

    fn scored(score: f32, raw: f32, prominent: bool) -> ScoredRegion {
        ScoredRegion {
            score,
            raw,
            prominent,
        }
    }

    #[test]
    fn one_confident_image_is_held_under_high() {
        let (risk, nsfw) = combine(&[scored(1.0, 1.0, false)]);
        assert_eq!(nsfw, Some(1.0));
        assert_eq!(risk, BELOW_HIGH);
    }

    #[test]
    fn three_agreeing_images_reach_high() {
        let three = [
            scored(1.0, 1.0, false),
            scored(0.9, 0.95, false),
            scored(0.8, 0.91, false),
        ];
        assert_eq!(combine(&three), (1.0, Some(1.0)));
    }

    #[test]
    fn one_prominent_image_reaches_high_only_if_confident() {
        assert_eq!(combine(&[scored(1.0, 1.0, true)]), (1.0, Some(1.0)));
        // Prominent, but below the agreement bar.
        let (risk, _) = combine(&[scored(0.5, 0.85, true)]);
        assert!(risk <= BELOW_HIGH, "{risk}");
    }

    #[test]
    fn risk_is_the_best_region_score_without_skin() {
        let regions = [
            scored(0.3, 0.9, true),
            scored(0.6, 0.95, false),
            scored(0.0, 0.5, false),
        ];
        assert_eq!(combine(&regions), (0.6, Some(0.6)));
    }

    #[test]
    fn no_scored_regions_means_no_risk() {
        assert_eq!(combine(&[]), (0.0, None));
    }

    /// The Monk Skin Tone scale, lightest to darkest.
    const MONK: [[u8; 3]; 10] = [
        [0xf6, 0xed, 0xe4],
        [0xf3, 0xe7, 0xdb],
        [0xf7, 0xea, 0xd0],
        [0xea, 0xda, 0xba],
        [0xd7, 0xbd, 0x96],
        [0xa0, 0x7e, 0x56],
        [0x82, 0x5c, 0x43],
        [0x60, 0x41, 0x34],
        [0x3a, 0x31, 0x2a],
        [0x29, 0x24, 0x20],
    ];

    #[test]
    fn every_monk_skin_tone_counts_as_skin() {
        for [r, g, b] in MONK {
            assert!(
                is_skin(r, g, b, true),
                "#{r:02x}{g:02x}{b:02x} should be skin"
            );
        }
    }

    #[test]
    fn whole_frame_scoring_keeps_the_ycbcr_rule() {
        // `classify_image` doesn't use the dark-skin rule, so the three darkest tones don't
        // count there.
        for [r, g, b] in &MONK[7..] {
            assert!(!is_skin(*r, *g, *b, false));
        }
        assert!(MONK[..7].iter().all(|&[r, g, b]| is_skin(r, g, b, false)));
    }

    #[test]
    fn a_model_that_fails_to_build_falls_back_to_skin() {
        // A plain skin-toned frame has no images, so the whole frame is scored, and its skin
        // passes the gate; the model can't be built, so the risk is the skin contribution.
        let classifier = RiskClassifier::new(b"not a model");
        let result = classifier
            .classify_regions(&solid(240, 200, 160), Screen::Desktop)
            .unwrap();
        assert!(result.regions.is_empty());
        assert_eq!(result.model_runs, 0);
        assert_eq!(result.scores.nsfw, None);
        assert_eq!(result.scores.risk, SKIN_WEIGHT);
    }

    #[test]
    fn warm_dark_theme_colors_pass_only_the_per_image_skin_gate() {
        // Gruvbox bg2, a warm gray, Material brown 700 and 800, dark khaki. They overlap the
        // darkest Monk tones, so the dark rule counts them; whole-frame scoring, where skin
        // adds to the risk, doesn't use that rule.
        for [r, g, b] in [
            [0x50, 0x49, 0x45],
            [0x48, 0x40, 0x3a],
            [0x5d, 0x40, 0x37],
            [0x4e, 0x34, 0x2e],
            [0x55, 0x4b, 0x3c],
        ] {
            assert!(is_skin(r, g, b, true), "#{r:02x}{g:02x}{b:02x}");
            assert!(!is_skin(r, g, b, false), "#{r:02x}{g:02x}{b:02x}");
        }
    }

    #[test]
    fn dark_ui_colors_are_not_skin() {
        // Black, dark grays and the backgrounds of common dark themes.
        for [r, g, b] in [
            [0x00, 0x00, 0x00],
            [0x1a, 0x16, 0x14],
            [0x1e, 0x1e, 0x1e],
            [0x33, 0x33, 0x33],
            [0x4d, 0x4d, 0x4d],
            [0x28, 0x2c, 0x34],
            [0x36, 0x39, 0x3f],
            [0x0d, 0x11, 0x17],
        ] {
            assert!(
                !is_skin(r, g, b, true),
                "#{r:02x}{g:02x}{b:02x} should not be skin"
            );
        }
    }
}
