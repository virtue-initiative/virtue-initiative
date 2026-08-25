use image::{DynamicImage, GenericImageView};

use crate::error::CoreResult;
use crate::model::Screenshot;

const TARGET_SMALL_DIM: u32 = 128;
// Blurred post-resize (see below), so this reads as a much stronger blur than the
// same sigma would at full resolution — kept low so large color/shape regions (the
// signal needed to recognize NSFW content at a glance) stay intact; only fine detail
// like on-screen text is meant to be destroyed.
const BLUR_SIGMA: f32 = 1.0;
const WEBP_QUALITY: f32 = 1.0;

#[derive(Debug, Clone, Default)]
pub struct ImagePipeline;

impl ImagePipeline {
    pub fn process(&self, screenshot: Screenshot) -> CoreResult<Screenshot> {
        let decoded = image::load_from_memory(&screenshot.bytes)?;
        self.process_image(decoded, screenshot.captured_at_ms)
    }

    /// Same as [`process`](Self::process), but takes an already-decoded image so a caller that
    /// decoded the screenshot earlier (e.g. for the fingerprint/classifier, or to redact text)
    /// doesn't pay for yet another full-resolution PNG decode of the same frame.
    pub fn process_image(
        &self,
        decoded: DynamicImage,
        captured_at_ms: i64,
    ) -> CoreResult<Screenshot> {
        let (orig_width, orig_height) = decoded.dimensions();
        let scale = TARGET_SMALL_DIM as f32 / orig_width.min(orig_height) as f32;
        let target_width = (orig_width as f32 * scale).round().max(1.0) as u32;
        let target_height = (orig_height as f32 * scale).round().max(1.0) as u32;
        // Resize down to the thumbnail size *before* blurring: `blur` allocates
        // two full-image f32 scratch buffers (see `image::imageops::sample::
        // gaussian_blur_indirect_impl`), which at full screenshot resolution
        // dwarfs every other allocation in this pipeline (~144MB for a typical
        // screenshot vs. ~20MB this way). Blurring at full res first also isn't
        // required for the intended effect: text/detail is already destroyed by
        // the lossy WebP encode below, and large color/shape regions (the signal
        // needed to recognize NSFW content) survive resizing+blurring+encoding
        // either way.
        let resized = decoded.resize_exact(
            target_width,
            target_height,
            image::imageops::FilterType::Lanczos3,
        );
        let blurred = resized.blur(BLUR_SIGMA);

        let rgba = blurred.to_rgba8();
        let (width, height) = blurred.dimensions();
        let encoded = webp::Encoder::from_rgba(rgba.as_raw(), width, height).encode(WEBP_QUALITY);

        Ok(Screenshot {
            captured_at_ms,
            bytes: encoded.to_vec(),
            content_type: "image/webp".to_string(),
        })
    }
}
