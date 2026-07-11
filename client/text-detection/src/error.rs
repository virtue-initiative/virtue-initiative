/// Errors returned by [`crate::ScreenshotOCR`].
#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("OCR engine initialization failed: {0}")]
    Init(String),
    #[error("image loading failed: {0}")]
    ImageLoad(String),
    #[error("text recognition failed: {0}")]
    Recognition(String),
    #[error("OCR is not yet implemented on this platform")]
    Unimplemented,
}
