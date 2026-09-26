use image::{DynamicImage, GenericImageView};

use crate::error::CoreResult;
use crate::model::Screenshot;

// Every capture is shrunk by the same factor, so each monitor keeps the same
// detail however many there are and however they're arranged: one 1920x1080
// screen becomes 240x135, and two of them side by side become 480x135.
const DOWNSCALE_FACTOR: u32 = 8;
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
        let (target_width, target_height) = target_dimensions(orig_width, orig_height);
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

fn target_dimensions(width: u32, height: u32) -> (u32, u32) {
    let shrink = |dim: u32| dim.div_ceil(DOWNSCALE_FACTOR).max(1);
    (shrink(width), shrink(height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_dimensions_scale_with_the_original() {
        assert_eq!(target_dimensions(1920, 1080), (240, 135));
        assert_eq!(target_dimensions(2560, 1440), (320, 180));
    }

    #[test]
    fn target_dimensions_give_each_monitor_the_same_detail_in_any_layout() {
        // Side by side, stacked, and mixed sizes: each monitor's share of the
        // output matches what it would get on its own.
        assert_eq!(target_dimensions(3840, 1080), (480, 135));
        assert_eq!(target_dimensions(1920, 2160), (240, 270));
        assert_eq!(target_dimensions(1920 + 2560, 1440), (240 + 320, 180));
    }

    #[test]
    fn target_dimensions_never_collapse_to_zero() {
        assert_eq!(target_dimensions(1, 1), (1, 1));
        assert_eq!(target_dimensions(9, 3), (2, 1));
    }

    #[test]
    fn process_image_outputs_webp_at_target_dimensions() {
        let decoded = DynamicImage::new_rgba8(3840, 1080);
        let out = ImagePipeline.process_image(decoded, 42).unwrap();
        assert_eq!(out.content_type, "image/webp");
        assert_eq!(out.captured_at_ms, 42);
        let webp = webp::Decoder::new(&out.bytes).decode().unwrap();
        assert_eq!((webp.width(), webp.height()), (480, 135));
    }
}
