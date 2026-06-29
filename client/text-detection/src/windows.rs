// Windows.Media.Ocr does not expose per-word or per-line confidence scores,
// so min_word_confidence is ignored on Windows.

use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

use crate::{BoundingBox, OcrError, OcrOptions, OcrResult, TextRegion};

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
        // 1. Decode to RGBA8, then swap R↔B to produce BGRA (required by SoftwareBitmap).
        let img = image::load_from_memory(image)
            .map_err(|e| OcrError::ImageLoad(e.to_string()))?
            .to_rgba8();
        let (width, height) = img.dimensions();

        let mut bgra = img.into_raw();
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }

        // 2. Build SoftwareBitmap from raw pixel bytes via DataWriter.
        let writer = DataWriter::new().map_err(|e| OcrError::Init(e.to_string()))?;
        writer
            .WriteBytes(&bgra)
            .map_err(|e| OcrError::Init(e.to_string()))?;
        let buf = writer
            .DetachBuffer()
            .map_err(|e| OcrError::Init(e.to_string()))?;
        let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
            &buf,
            BitmapPixelFormat::Bgra8,
            width as i32,
            height as i32,
            BitmapAlphaMode::Premultiplied,
        )
        .map_err(|e| OcrError::Init(e.to_string()))?;

        // 3. Create the OCR engine (language-locked at construction time).
        let engine = if let Some(lang) = &self.language {
            let lang_hstring: windows::core::HSTRING = lang.as_str().into();
            let language = Language::CreateLanguage(&lang_hstring)
                .map_err(|e| OcrError::Init(e.to_string()))?;
            OcrEngine::TryCreateFromLanguage(&language)
                .map_err(|e| OcrError::Init(e.to_string()))?
        } else {
            OcrEngine::TryCreateFromUserProfileLanguages()
                .map_err(|e| OcrError::Init(e.to_string()))?
        };
        let engine =
            engine.ok_or_else(|| OcrError::Init("no OCR engine available for language".into()))?;

        // 4. Recognize — blocks the calling thread (don't call from a UI thread).
        let result = engine
            .RecognizeAsync(&bitmap)
            .map_err(|e| OcrError::Recognition(e.to_string()))?
            .get()
            .map_err(|e| OcrError::Recognition(e.to_string()))?;

        // 5. Collect word-level regions; BoundingRect is already pixel coords, top-left origin.
        let mut regions = Vec::new();
        let mut line_texts = Vec::new();

        for line in result
            .Lines()
            .map_err(|e| OcrError::Recognition(e.to_string()))?
        {
            let mut word_texts = Vec::new();
            for word in line
                .Words()
                .map_err(|e| OcrError::Recognition(e.to_string()))?
            {
                let text = word
                    .Text()
                    .map_err(|e| OcrError::Recognition(e.to_string()))?
                    .to_string();
                let rect = word
                    .BoundingRect()
                    .map_err(|e| OcrError::Recognition(e.to_string()))?;
                if text.trim().is_empty() {
                    continue;
                }
                regions.push(TextRegion {
                    text: text.clone(),
                    bounding_box: BoundingBox {
                        x: rect.X,
                        y: rect.Y,
                        width: rect.Width,
                        height: rect.Height,
                    },
                });
                word_texts.push(text);
            }
            if !word_texts.is_empty() {
                line_texts.push(word_texts.join(" "));
            }
        }

        Ok(OcrResult {
            regions,
            text: line_texts.join("\n"),
        })
    }
}
