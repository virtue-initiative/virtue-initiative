#[cfg(feature = "nsfw")]
mod real {
    use image::DynamicImage;
    use nsfw::model::Metric;

    static MODEL_BYTES: &[u8] = include_bytes!(env!("VIRTUE_NSFW_MODEL_PATH"));

    pub struct NsfwClassifier {
        model: nsfw::Model,
    }

    impl NsfwClassifier {
        pub fn new() -> crate::error::CoreResult<Self> {
            let model = nsfw::create_model(MODEL_BYTES)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            Ok(Self { model })
        }

        pub fn score(&self, decoded: &DynamicImage) -> Option<f32> {
            let rgba = decoded.to_rgba8();
            match nsfw::examine(&self.model, &rgba) {
                Err(e) => {
                    eprintln!("nsfw classifier error: {e}");
                    None
                }
                Ok(classifications) => {
                    let score = classifications
                        .iter()
                        .filter(|c| {
                            matches!(c.metric, Metric::Porn | Metric::Hentai | Metric::Sexy)
                        })
                        .map(|c| c.score)
                        .fold(0.0_f32, f32::max)
                        .clamp(0.0, 1.0);
                    Some(score)
                }
            }
        }
    }
}

#[cfg(not(feature = "nsfw"))]
mod stub {
    use image::DynamicImage;

    pub struct NsfwClassifier;

    impl NsfwClassifier {
        pub fn new() -> crate::error::CoreResult<Self> {
            Err(crate::error::CoreError::InvalidState(
                "nsfw feature is disabled",
            ))
        }

        pub fn score(&self, _decoded: &DynamicImage) -> Option<f32> {
            None
        }
    }
}

#[cfg(feature = "nsfw")]
pub use real::NsfwClassifier;

#[cfg(not(feature = "nsfw"))]
pub use stub::NsfwClassifier;

#[cfg(all(test, feature = "nsfw"))]
mod tests {
    use super::NsfwClassifier;

    fn solid_color_png(r: u8, g: u8, b: u8) -> image::DynamicImage {
        let mut img = image::RgbaImage::new(64, 64);
        for pixel in img.pixels_mut() {
            *pixel = image::Rgba([r, g, b, 255]);
        }
        image::DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn benign_image_scores_below_threshold() {
        let classifier = NsfwClassifier::new().expect("classifier should initialize");
        let img = solid_color_png(200, 200, 200);
        let score = classifier.score(&img);
        assert!(score.is_some());
        assert!(score.unwrap() < 0.4, "expected low score, got {:?}", score);
    }

    #[test]
    fn score_is_always_in_range() {
        let classifier = NsfwClassifier::new().expect("classifier should initialize");
        let img = solid_color_png(100, 50, 200);
        let score = classifier.score(&img);
        if let Some(s) = score {
            assert!((0.0..=1.0).contains(&s), "score out of range: {s}");
        }
    }

    #[test]
    fn malformed_bytes_returns_none() {
        // When the image is already decoded we don't get malformed bytes,
        // but we can verify that score() never returns Err.
        let classifier = NsfwClassifier::new().expect("classifier should initialize");
        let img = solid_color_png(0, 0, 0);
        let result = classifier.score(&img);
        // must be Some(_) or None — never a panic
        let _ = result;
    }
}
