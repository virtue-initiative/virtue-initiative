use image::{DynamicImage, GenericImageView};

use crate::error::CoreResult;
use crate::upload::sha256_hex;

const WEBP_QUALITY: f32 = 80.0;
const RESIZE_FACTOR: f32 = 1.0;

#[derive(Clone, Debug)]
pub struct ImagePipelineConfig {
    pub max_width: u32,
    pub max_height: u32,
    pub resize_factor: f32,
    pub webp_quality: f32,
}

impl Default for ImagePipelineConfig {
    fn default() -> Self {
        Self {
            max_width: 1280,
            max_height: 720,
            resize_factor: RESIZE_FACTOR,
            webp_quality: WEBP_QUALITY,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProcessedImage {
    pub bytes: Vec<u8>,
    pub sha256_hex: String,
    pub content_type: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Default)]
pub struct ImagePipeline {
    config: ImagePipelineConfig,
}

impl ImagePipeline {
    pub fn new(config: ImagePipelineConfig) -> Self {
        Self { config }
    }

    pub fn process(&self, input: &[u8]) -> CoreResult<ProcessedImage> {
        let decoded = image::load_from_memory(input)?;
        let capped = decoded.resize(
            self.config.max_width,
            self.config.max_height,
            image::imageops::FilterType::Lanczos3,
        );

        let scaled_width = ((capped.width() as f32) * self.config.resize_factor).round() as u32;
        let scaled_height = ((capped.height() as f32) * self.config.resize_factor).round() as u32;
        let working = if scaled_width != capped.width() || scaled_height != capped.height() {
            capped.resize_exact(scaled_width, scaled_height, image::imageops::FilterType::Lanczos3)
        } else {
            capped
        };

        let bytes = encode_webp(&working, self.config.webp_quality)?;
        Ok(ProcessedImage {
            sha256_hex: sha256_hex(&bytes),
            content_type: "image/webp".to_string(),
            width: working.width(),
            height: working.height(),
            bytes,
        })
    }
}

fn encode_webp(image: &DynamicImage, quality: f32) -> CoreResult<Vec<u8>> {
    let rgba = image.to_rgba8();
    let (width, height) = image.dimensions();
    let encoded = webp::Encoder::from_rgba(rgba.as_raw(), width, height)
        .encode(quality);
    Ok(encoded.to_vec())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

    use super::{ImagePipeline, ImagePipelineConfig};

    #[test]
    fn webp_pipeline_encodes_output() {
        let mut source = RgbImage::new(800, 600);
        for pixel in source.pixels_mut() {
            *pixel = Rgb([22, 44, 88]);
        }

        let mut input = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(source)
            .write_to(&mut input, ImageFormat::Png)
            .expect("encode input");

        let pipeline = ImagePipeline::default();
        let output = pipeline
            .process(&input.into_inner())
            .expect("process webp");

        assert_eq!(output.content_type, "image/webp");
        assert!(!output.bytes.is_empty());
    }

    #[test]
    fn webp_pipeline_respects_resize_factor() {
        let mut source = RgbImage::new(800, 600);
        for pixel in source.pixels_mut() {
            *pixel = Rgb([22, 44, 88]);
        }

        let mut input = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(source)
            .write_to(&mut input, ImageFormat::Png)
            .expect("encode input");

        let config = ImagePipelineConfig {
            resize_factor: 0.5,
            ..ImagePipelineConfig::default()
        };
        let pipeline = ImagePipeline::new(config);
        let output = pipeline
            .process(&input.into_inner())
            .expect("process webp");

        assert_eq!(output.width, 400);
        assert_eq!(output.height, 300);
    }
}
