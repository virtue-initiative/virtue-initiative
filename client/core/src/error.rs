use std::io;

use thiserror::Error;

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("messagepack encode error: {0}")]
    MessagePackEncode(#[from] rmp_serde::encode::Error),
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("argon2 error: {0}")]
    Argon2(String),
    #[error("request failed with status {status}: {message}")]
    HttpStatus { status: u16, message: String },
    #[error("service has not been authenticated")]
    NotAuthenticated,
    #[error("service has already been shut down")]
    Shutdown,
    #[error("invalid state: {0}")]
    InvalidState(&'static str),
    #[error("crypto error: {0}")]
    Crypto(&'static str),
    #[error("external command failed: {0}")]
    CommandFailed(String),
}

impl CoreError {
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::HttpStatus { status: 401, .. })
    }

    pub fn is_bad_request(&self) -> bool {
        matches!(self, Self::HttpStatus { status: 400, .. })
    }
}
