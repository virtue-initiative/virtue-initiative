use image::GenericImageView;

use crate::error::CoreResult;
use crate::model::Screenshot;

const TARGET_SMALL_DIM: u32 = 128;
const BLUR_SIGMA: f32 = 2.0;
const WEBP_QUALITY: f32 = 1.0;

#[derive(Debug, Clone, Default)]
pub struct ImagePipeline;

impl ImagePipeline {
    pub fn process(&self, screenshot: Screenshot) -> CoreResult<Screenshot> {
        let decoded = image::load_from_memory(&screenshot.bytes)?;
        let blurred = decoded.blur(BLUR_SIGMA);
        let (orig_width, orig_height) = blurred.dimensions();
        let scale = TARGET_SMALL_DIM as f32 / orig_width.min(orig_height) as f32;
        let target_width = (orig_width as f32 * scale).round().max(1.0) as u32;
        let target_height = (orig_height as f32 * scale).round().max(1.0) as u32;
        let resized = blurred.resize_exact(
            target_width,
            target_height,
            image::imageops::FilterType::Lanczos3,
        );

        let rgba = resized.to_rgba8();
        let (width, height) = resized.dimensions();
        let encoded = webp::Encoder::from_rgba(rgba.as_raw(), width, height).encode(WEBP_QUALITY);

        Ok(Screenshot {
            captured_at_ms: screenshot.captured_at_ms,
            bytes: encoded.to_vec(),
            content_type: "image/webp".to_string(),
        })
    }
}
