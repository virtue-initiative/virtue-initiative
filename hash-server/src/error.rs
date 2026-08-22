use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
}

/// The shapes of failure this server can produce, per HASH-002.
/// Every variant maps to one (status, code, message) triple.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("invalid body")]
    InvalidBody(Option<String>),
    #[error("invalid query")]
    InvalidQuery(Option<String>),
    #[error("unauthorized")]
    Unauthorized(Option<String>),
    #[error("forbidden")]
    Forbidden(Option<String>),
    #[error("sequence conflict")]
    SequenceConflict,
    #[error("internal error")]
    Internal(Option<String>),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message, details) = match self {
            ApiError::InvalidBody(details) => (
                StatusCode::BAD_REQUEST,
                "invalid_body",
                "The request contains an invalid body",
                details,
            ),
            ApiError::InvalidQuery(details) => (
                StatusCode::BAD_REQUEST,
                "invalid_query",
                "The request contains an invalid or malformed query parameter",
                details,
            ),
            ApiError::Unauthorized(details) => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "The request is not authorized",
                details,
            ),
            ApiError::Forbidden(details) => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "The token does not authorize this device",
                details,
            ),
            ApiError::SequenceConflict => (
                StatusCode::CONFLICT,
                "sequence_conflict",
                "The sequence number is not strictly greater than the previous one",
                None,
            ),
            ApiError::Internal(details) => {
                // HASH-014: every unexpected (5xx) error is logged.
                tracing::error!(details = details.as_deref().unwrap_or(""), "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "An internal error occurred",
                    details,
                )
            }
        };

        (
            status,
            Json(ErrorBody {
                code,
                message,
                details,
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    // HASH-014: "The server SHOULD log every unexpected error (5xx codes)."
    #[traced_test]
    #[test]
    fn internal_errors_are_logged() {
        let _ = ApiError::Internal(Some("boom".into())).into_response();
        assert!(logs_contain("internal error"));
    }

    #[traced_test]
    #[test]
    fn non_5xx_errors_are_not_logged_as_internal() {
        let _ = ApiError::Unauthorized(None).into_response();
        let _ = ApiError::SequenceConflict.into_response();
        assert!(!logs_contain("internal error"));
    }
}
