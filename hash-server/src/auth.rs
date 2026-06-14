use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    #[serde(rename = "type")]
    typ: String,
}

fn make_validation() -> Validation {
    let mut v = Validation::new(Algorithm::EdDSA);
    v.validate_aud = false;
    v.validate_exp = true;
    v
}

fn extract_token(parts: &Parts) -> Result<String, AuthError> {
    let header = parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(AuthError::Missing)?;

    header
        .strip_prefix("Bearer ")
        .map(|t| t.to_owned())
        .ok_or(AuthError::Missing)
}

fn verify(token: &str, key: &DecodingKey, required_type: &str) -> Result<String, AuthError> {
    let data = decode::<Claims>(token, key, &make_validation())
        .map_err(|_| AuthError::Invalid)?;

    if data.claims.typ != required_type {
        return Err(AuthError::WrongType);
    }

    Ok(data.claims.sub)
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("missing authorization header")]
    Missing,
    #[error("invalid or expired token")]
    Invalid,
    #[error("wrong token type")]
    WrongType,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            AuthError::Missing => (StatusCode::UNAUTHORIZED, json!({"error": "Unauthorized"})),
            AuthError::Invalid => (
                StatusCode::UNAUTHORIZED,
                json!({"error": "Unauthorized", "details": {"reason": "Invalid or expired token"}}),
            ),
            AuthError::WrongType => (
                StatusCode::UNAUTHORIZED,
                json!({"error": "Unauthorized", "details": {"reason": "Invalid token type"}}),
            ),
        };
        (status, axum::Json(body)).into_response()
    }
}

pub struct DeviceAccessAuth(pub String);

impl FromRequestParts<Arc<AppState>> for DeviceAccessAuth {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_token(parts)?;
        let device_id = verify(&token, &state.decoding_key, "device-access")?;
        Ok(DeviceAccessAuth(device_id))
    }
}

pub struct ServerAuth(pub String);

impl FromRequestParts<Arc<AppState>> for ServerAuth {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_token(parts)?;
        let device_id = verify(&token, &state.decoding_key, "server")?;
        Ok(ServerAuth(device_id))
    }
}

pub struct DeviceAccessOrServerAuth(pub String);

impl FromRequestParts<Arc<AppState>> for DeviceAccessOrServerAuth {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_token(parts)?;
        let data = jsonwebtoken::decode::<Claims>(&token, &state.decoding_key, &make_validation())
            .map_err(|_| AuthError::Invalid)?;
        if data.claims.typ == "device-access" || data.claims.typ == "server" {
            Ok(DeviceAccessOrServerAuth(data.claims.sub))
        } else {
            Err(AuthError::WrongType)
        }
    }
}
