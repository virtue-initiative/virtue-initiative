use reqwest::StatusCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("token store error: {0}")]
    TokenStore(String),

    #[error("queue lock poisoned")]
    QueueLockPoisoned,

    #[error("unexpected response ({status}): {body}")]
    UnexpectedResponse { status: StatusCode, body: String },

    #[error("checksum mismatch: expected {expected}, actual {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("time conversion error: {0}")]
    Time(String),

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("serialization error: {0}")]
    Serialization(String),
}

pub type CoreResult<T> = Result<T, CoreError>;
