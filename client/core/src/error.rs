use std::io;

use thiserror::Error;

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("serialization error in {context}: {source}")]
    SerdeContext {
        context: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("messagepack encode error: {0}")]
    MessagePackEncode(#[from] rmp_serde::encode::Error),
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("HTTP error: {0}")]
    Http(#[from] ureq::Error),
    #[error("base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("argon2 error: {0}")]
    Argon2(String),
    #[error("{message}")]
    HttpStatus { status: u16, message: String },
    /// API-038: the batch begins with events the server already stored, whose
    /// response was lost. The hash chain now starts at `first_seq` (0 if not
    /// known) and ends at `seq` (0 if empty).
    #[error(
        "the server already stored this batch's first events (its chain is at {first_seq}..={seq})"
    )]
    BatchAlreadyStored { first_seq: u32, seq: u32 },
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
    #[error("{0}")]
    Remote(String),
    #[error("classifier error: {0}")]
    Classifier(String),
    #[error("IPC error: {0}")]
    Ipc(String),
}

impl CoreError {
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::HttpStatus { status: 401, .. })
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::HttpStatus { status: 404, .. })
    }

    pub fn is_bad_request(&self) -> bool {
        matches!(self, Self::HttpStatus { status: 400, .. })
    }

    pub fn is_conflict(&self) -> bool {
        matches!(self, Self::HttpStatus { status: 409, .. })
    }
}
