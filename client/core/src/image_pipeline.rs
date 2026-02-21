use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::webp::WebPEncoder;
use image::{DynamicImage, GenericImageView, ImageEncoder};

use crate::error::CoreResult;
use crate::upload::sha256_hex;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageOutputFormat {
    Jpeg,
    Webp,
}

impl ImageOutputFormat {
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ImagePipelineConfig {
    pub target_max_bytes: usize,
    pub max_width: u32,
    pub max_height: u32,
    pub min_width: u32,
    pub min_height: u32,
    pub initial_jpeg_quality: u8,
    pub min_jpeg_quality: u8,
    pub jpeg_quality_step: u8,
    pub downscale_ratio: f32,
    pub max_iterations: usize,
}

impl Default for ImagePipelineConfig {
    fn default() -> Self {
        Self {
            target_max_bytes: 5 * 1024,
            max_width: 1280,
            max_height: 720,
            min_width: 200,
            min_height: 120,
            initial_jpeg_quality: 82,
            min_jpeg_quality: 30,
            jpeg_quality_step: 8,
            downscale_ratio: 0.85,
            max_iterations: 14,
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

    pub fn process(&self, input: &[u8], format: ImageOutputFormat) -> CoreResult<ProcessedImage> {
        let decoded = image::load_from_memory(input)?;
        let mut working = decoded.resize(
            self.config.max_width,
            self.config.max_height,
            image::imageops::FilterType::Lanczos3,
        );

        let mut jpeg_quality = self.config.initial_jpeg_quality;
        let mut best_candidate = None;

        for _ in 0..self.config.max_iterations {
            let encoded = encode(&working, format, jpeg_quality)?;

            if best_candidate
                .as_ref()
                .map(|candidate: &Vec<u8>| encoded.len() < candidate.len())
                .unwrap_or(true)
            {
                best_candidate = Some(encoded.clone());
            }

            if encoded.len() <= self.config.target_max_bytes {
                return Ok(finalize_image(
                    encoded,
                    format,
                    working.width(),
                    working.height(),
                ));
            }

            match format {
                ImageOutputFormat::Jpeg => {
                    if jpeg_quality > self.config.min_jpeg_quality {
                        jpeg_quality = jpeg_quality.saturating_sub(self.config.jpeg_quality_step);
                        if jpeg_quality < self.config.min_jpeg_quality {
                            jpeg_quality = self.config.min_jpeg_quality;
                        }
                    } else {
                        working = downscale(
                            &working,
                            self.config.downscale_ratio,
                            self.config.min_width,
                            self.config.min_height,
                        );
                    }
                }
                ImageOutputFormat::Webp => {
                    working = downscale(
                        &working,
                        self.config.downscale_ratio,
                        self.config.min_width,
                        self.config.min_height,
                    );
                }
            }
        }

        let fallback = match best_candidate {
            Some(candidate) => candidate,
            None => encode(&working, format, jpeg_quality)?,
        };

        Ok(finalize_image(
            fallback,
            format,
            working.width(),
            working.height(),
        ))
    }
}

fn finalize_image(
    bytes: Vec<u8>,
    format: ImageOutputFormat,
    width: u32,
    height: u32,
) -> ProcessedImage {
    ProcessedImage {
        sha256_hex: sha256_hex(&bytes),
        bytes,
        content_type: format.content_type().to_string(),
        width,
        height,
    }
}

fn downscale(image: &DynamicImage, ratio: f32, min_width: u32, min_height: u32) -> DynamicImage {
    let next_width = ((image.width() as f32) * ratio).round() as u32;
    let next_height = ((image.height() as f32) * ratio).round() as u32;

    let width = next_width.max(min_width).min(image.width());
    let height = next_height.max(min_height).min(image.height());

    if width == image.width() && height == image.height() {
        return image.clone();
    }

    image.resize_exact(width, height, image::imageops::FilterType::Triangle)
}

fn encode(
    image: &DynamicImage,
    format: ImageOutputFormat,
    jpeg_quality: u8,
) -> CoreResult<Vec<u8>> {
    match format {
        ImageOutputFormat::Jpeg => encode_jpeg(image, jpeg_quality),
        ImageOutputFormat::Webp => encode_webp(image),
    }
}

fn encode_jpeg(image: &DynamicImage, quality: u8) -> CoreResult<Vec<u8>> {
    let mut output = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut output, quality);
    encoder.encode_image(image)?;
    Ok(output)
}

fn encode_webp(image: &DynamicImage) -> CoreResult<Vec<u8>> {
    let rgba = image.to_rgba8();
    let (width, height) = image.dimensions();

    let mut output = Cursor::new(Vec::<u8>::new());
    let encoder = WebPEncoder::new_lossless(&mut output);
    encoder.write_image(
        rgba.as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )?;

    Ok(output.into_inner())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

    use super::{ImageOutputFormat, ImagePipeline, ImagePipelineConfig};

    #[test]
    fn jpeg_pipeline_targets_size_budget() {
        let mut source = RgbImage::new(1600, 1000);
        for pixel in source.pixels_mut() {
            *pixel = Rgb([120, 180, 200]);
        }

        let mut png_bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(source)
            .write_to(&mut png_bytes, ImageFormat::Png)
            .expect("encode png");

        let config = ImagePipelineConfig {
            target_max_bytes: 5 * 1024,
            ..ImagePipelineConfig::default()
        };
        let pipeline = ImagePipeline::new(config.clone());

        let output = pipeline
            .process(&png_bytes.into_inner(), ImageOutputFormat::Jpeg)
            .expect("process jpeg");

        assert!(output.bytes.len() <= config.target_max_bytes);
        assert_eq!(output.content_type, "image/jpeg");
    }

    #[test]
    fn webp_pipeline_strips_to_encoded_output() {
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
            .process(&input.into_inner(), ImageOutputFormat::Webp)
            .expect("process webp");

        assert_eq!(output.content_type, "image/webp");
        assert!(!output.bytes.is_empty());
    }
}
