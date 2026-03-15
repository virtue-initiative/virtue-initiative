use image::GenericImageView;

use crate::error::CoreResult;
use crate::upload::sha256_hex;

const TARGET_SMALL_DIM: u32 = 128;
const BLUR_SIGMA: f32 = 2.0;
const WEBP_QUALITY: f32 = 1.0;

#[derive(Clone, Debug)]
pub struct ProcessedImage {
    pub bytes: Vec<u8>,
    pub sha256_hex: String,
    pub content_type: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Default)]
pub struct ImagePipeline;

impl ImagePipeline {
    pub fn process(&self, input: &[u8]) -> CoreResult<ProcessedImage> {
        let decoded = image::load_from_memory(input)?;
        let blurred = decoded.blur(BLUR_SIGMA);
        let (orig_width, orig_height) = blurred.dimensions();
        let scale = TARGET_SMALL_DIM as f32 / orig_width.min(orig_height) as f32;
        let target_width = (orig_width as f32 * scale).round() as u32;
        let target_height = (orig_height as f32 * scale).round() as u32;
        let working = blurred.resize_exact(
            target_width.max(1),
            target_height.max(1),
            image::imageops::FilterType::Lanczos3,
        );

        let rgba = working.to_rgba8();
        let (width, height) = working.dimensions();
        let encoded = webp::Encoder::from_rgba(rgba.as_raw(), width, height).encode(WEBP_QUALITY);
        let bytes = encoded.to_vec();

        Ok(ProcessedImage {
            sha256_hex: sha256_hex(&bytes),
            content_type: "image/webp".to_string(),
            width,
            height,
            bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

    use super::{ImagePipeline, TARGET_SMALL_DIM};

    #[test]
    fn resizes_so_small_dimension_matches_target() {
        let mut source = RgbImage::new(800, 600);
        for pixel in source.pixels_mut() {
            *pixel = Rgb([22, 44, 88]);
        }

        let mut input = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(source)
            .write_to(&mut input, ImageFormat::Png)
            .expect("encode input");

        let output = ImagePipeline.process(&input.into_inner()).expect("process");
        let expected_width = (800.0_f32 / 600.0_f32 * TARGET_SMALL_DIM as f32).round() as u32;

        assert_eq!(output.height, TARGET_SMALL_DIM);
        assert_eq!(output.width, expected_width);
        assert_eq!(output.content_type, "image/webp");
        assert!(!output.bytes.is_empty());
    }

    #[test]
    fn resizes_portrait_images_using_width_as_small_dimension() {
        let mut source = RgbImage::new(600, 800);
        for pixel in source.pixels_mut() {
            *pixel = Rgb([22, 44, 88]);
        }

        let mut input = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(source)
            .write_to(&mut input, ImageFormat::Png)
            .expect("encode input");

        let output = ImagePipeline.process(&input.into_inner()).expect("process");
        let expected_height = (800.0_f32 / 600.0_f32 * TARGET_SMALL_DIM as f32).round() as u32;

        assert_eq!(output.width, TARGET_SMALL_DIM);
        assert_eq!(output.height, expected_height);
        assert_eq!(output.content_type, "image/webp");
        assert!(!output.bytes.is_empty());
    }
}
