use objc2::AnyThread;
use objc2_foundation::{NSArray, NSData, NSDictionary, NSString};
use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequestTextRecognitionLevel};

use crate::{BoundingBox, OcrError, OcrOptions, OcrResult, TextRegion};

pub struct ScreenshotOCR {
    language: Option<String>,
    min_confidence: u8,
}

impl ScreenshotOCR {
    pub fn new(options: OcrOptions) -> Result<Self, OcrError> {
        Ok(Self {
            language: options.language,
            min_confidence: options.min_word_confidence,
        })
    }

    pub fn detect(&self, image: &[u8]) -> Result<OcrResult, OcrError> {
        // Decode to get pixel dimensions for bounding box coordinate conversion.
        let img = image::load_from_memory(image).map_err(|e| OcrError::ImageLoad(e.to_string()))?;
        let w = img.width() as f32;
        let h = img.height() as f32;

        let (regions, lines) = unsafe { self.run_vision(image, w, h) }?;
        Ok(OcrResult {
            regions,
            text: lines.join("\n"),
        })
    }

    /// # Safety
    /// Calls into the Objective-C / Vision framework runtime.
    unsafe fn run_vision(
        &self,
        image: &[u8],
        w: f32,
        h: f32,
    ) -> Result<(Vec<TextRegion>, Vec<String>), OcrError> {
        // Wrap raw bytes in NSData (copies the slice).
        let ns_data = NSData::with_bytes(image);

        // Create handler — Vision auto-detects PNG/JPEG from the byte stream.
        let handler = VNImageRequestHandler::initWithData_options(
            VNImageRequestHandler::alloc(),
            &ns_data,
            &NSDictionary::new(),
        );

        // Build recognition request.
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);

        if let Some(lang) = &self.language {
            let ns_lang = NSString::from_str(lang);
            let langs: objc2::rc::Retained<NSArray<NSString>> =
                NSArray::from_slice(&[ns_lang.as_ref()]);
            request.setRecognitionLanguages(&langs);
        }

        // Run synchronously — Vision's perform is safe to call from any thread.
        let requests = NSArray::from_slice(&[request.as_ref()]);
        handler
            .performRequests_error(&requests)
            .map_err(|e| OcrError::Recognition(e.localizedDescription().to_string()))?;

        // Collect results.
        let observations = request
            .results()
            .ok_or_else(|| OcrError::Recognition("Vision returned no results".into()))?;

        let mut regions = Vec::new();
        let mut lines = Vec::new();

        for obs in observations.iter() {
            let candidates = obs.topCandidates(1);
            let Some(candidate) = candidates.firstObject() else {
                continue;
            };

            let text = candidate.string().to_string();
            if text.trim().is_empty() {
                continue;
            }

            // Vision bounding boxes: normalized ([0,1]), bottom-left origin.
            // Convert to pixel top-left coords.
            let bbox = unsafe { obs.boundingBox() };
            let region = TextRegion {
                text: text.clone(),
                bounding_box: BoundingBox {
                    x: bbox.origin.x as f32 * w,
                    y: (1.0 - bbox.origin.y as f32 - bbox.size.height as f32) * h,
                    width: bbox.size.width as f32 * w,
                    height: bbox.size.height as f32 * h,
                },
            };
            regions.push(region);

            // Vision confidence is per-observation (line-level), not per-word.
            // min_word_confidence is applied here at the line level.
            if unsafe { obs.confidence() } * 100.0 >= self.min_confidence as f32 {
                lines.push(text);
            }
        }

        Ok((regions, lines))
    }
}
