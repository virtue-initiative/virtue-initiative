mod error;
pub use error::OcrError;

/// Options passed to [`ScreenshotOCR::new`].
///
/// All fields have sensible defaults. Platform-specific fields are silently
/// ignored on platforms that don't use them.
#[derive(Debug, Clone)]
pub struct OcrOptions {
    /// BCP-47 language hint (e.g. `"eng"`, `"deu"`).  Defaults to `"eng"`.
    pub language: Option<String>,
    /// Linux/Tesseract only: path to the `tessdata/` directory. When `None`
    /// the system default (e.g. `/usr/share/tessdata`) is used.
    pub tesseract_data_path: Option<std::path::PathBuf>,
    /// Minimum Tesseract word confidence (0–100) for words included in
    /// `OcrResult::text`. Words below this threshold are dropped from the
    /// text output but still appear in `OcrResult::regions`.
    /// Default: 50. Set to 0 to keep everything.
    pub min_word_confidence: u8,
}

impl Default for OcrOptions {
    fn default() -> Self {
        Self {
            language: None,
            tesseract_data_path: None,
            min_word_confidence: 50,
        }
    }
}

/// Axis-aligned bounding box in pixel coordinates, measured from the
/// top-left corner of the image.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A single word or text fragment detected in the image.
#[derive(Debug, Clone)]
pub struct TextRegion {
    pub text: String,
    pub bounding_box: BoundingBox,
}

/// Combined result from a single OCR pass.
#[derive(Debug, Clone, Default)]
pub struct OcrResult {
    /// Word-level detections with pixel bounding boxes.
    pub regions: Vec<TextRegion>,
    /// Full page text with line and paragraph breaks, confidence-filtered.
    pub text: String,
}

// ── Platform dispatch ─────────────────────────────────────────────────────────
// Each platform module defines its own `ScreenshotOCR` struct with the same
// public API. Conditional compilation picks exactly one and re-exports it,
// so callers always write `ScreenshotOCR` with no trait objects or generics.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::ScreenshotOCR;

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use apple::ScreenshotOCR;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::ScreenshotOCR;

#[cfg(target_os = "android")]
pub mod android;
#[cfg(target_os = "android")]
pub use android::{ScreenshotOCR, register_recognize_fn};
