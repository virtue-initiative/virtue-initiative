use std::sync::{Arc, OnceLock};

use crate::{BoundingBox, OcrError, OcrOptions, OcrResult, TextRegion};

type RecognizeFn = Arc<dyn Fn(&[u8], Option<&str>) -> Result<String, OcrError> + Send + Sync>;
static OCR_FN: OnceLock<RecognizeFn> = OnceLock::new();

/// Register the OCR implementation for Android. Must be called before any
/// `ScreenshotOCR::detect()` — typically from JNI init.
pub fn register_recognize_fn<F>(f: F)
where
    F: Fn(&[u8], Option<&str>) -> Result<String, OcrError> + Send + Sync + 'static,
{
    let _ = OCR_FN.set(Arc::new(f));
}

pub struct ScreenshotOCR {
    language: Option<String>,
}

impl ScreenshotOCR {
    pub fn new(options: OcrOptions) -> Result<Self, OcrError> {
        Ok(Self {
            language: options.language,
        })
    }

    pub fn detect(&self, image: &[u8]) -> Result<OcrResult, OcrError> {
        let f = OCR_FN.get().ok_or_else(|| {
            OcrError::Init(
                "no OCR provider registered; call virtue_text_detection::android::register_recognize_fn at startup".into()
            )
        })?;
        let output = f(image, self.language.as_deref())?;
        parse_records(&output)
    }
}

fn parse_records(output: &str) -> Result<OcrResult, OcrError> {
    let mut regions = Vec::new();
    let mut text_lines = Vec::new();
    for record in output.split('\n') {
        if record.is_empty() {
            continue;
        }
        let parts: Vec<&str> = record.splitn(5, '|').collect();
        if parts.len() != 5 {
            continue;
        }
        let text = parts[0].to_string();
        if text.is_empty() {
            continue;
        }
        let left: f32 = parts[1].parse().unwrap_or(0.0);
        let top: f32 = parts[2].parse().unwrap_or(0.0);
        let right: f32 = parts[3].parse().unwrap_or(0.0);
        let bottom: f32 = parts[4].parse().unwrap_or(0.0);
        text_lines.push(text.clone());
        regions.push(TextRegion {
            text,
            bounding_box: BoundingBox {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            },
        });
    }
    Ok(OcrResult {
        regions,
        text: text_lines.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty() {
        let r = parse_records("").unwrap();
        assert!(r.regions.is_empty());
    }

    #[test]
    fn parse_single_region() {
        let r = parse_records("hello|10|20|110|40").unwrap();
        assert_eq!(r.regions.len(), 1);
        assert_eq!(r.regions[0].text, "hello");
        assert_eq!(r.regions[0].bounding_box.x, 10.0);
        assert_eq!(r.regions[0].bounding_box.width, 100.0);
        assert_eq!(r.text, "hello");
    }

    #[test]
    fn detect_with_mock_provider() {
        register_recognize_fn(|_image, _lang| Ok("foo|0|0|50|10".to_string()));
        let ocr = ScreenshotOCR::new(OcrOptions::default()).unwrap();
        let r = ocr.detect(&[]).unwrap();
        assert_eq!(r.regions[0].text, "foo");
    }
}
