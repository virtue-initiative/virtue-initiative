use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use dashmap::{DashMap, mapref::entry::Entry};
use ed25519_dalek::{Signature, VerifyingKey};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Clone, Deserialize)]
struct Claims {
    sub: String,
    #[serde(rename = "type")]
    typ: String,
    #[serde(default)]
    pubkey: Option<String>,
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

fn verify(token: &str, key: &DecodingKey, required_type: &str) -> Result<Claims, AuthError> {
    let data = decode::<Claims>(token, key, &make_validation())
        .map_err(|_| AuthError::Invalid)?;

    if data.claims.typ != required_type {
        return Err(AuthError::WrongType);
    }

    Ok(data.claims)
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("missing authorization header")]
    Missing,
    #[error("invalid or expired token")]
    Invalid,
    #[error("wrong token type")]
    WrongType,
    #[error("invalid or missing signature")]
    InvalidSignature,
    #[error("stale timestamp")]
    StaleTimestamp,
    #[error("replayed timestamp")]
    ReplayedTimestamp,
    #[error("invalid device pubkey")]
    InvalidPubkey,
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
            AuthError::InvalidSignature => (
                StatusCode::UNAUTHORIZED,
                json!({"error": "Unauthorized", "details": {"reason": "Invalid signature"}}),
            ),
            AuthError::StaleTimestamp => (
                StatusCode::UNAUTHORIZED,
                json!({"error": "Unauthorized", "details": {"reason": "Stale timestamp"}}),
            ),
            AuthError::ReplayedTimestamp => (
                StatusCode::UNAUTHORIZED,
                json!({"error": "Unauthorized", "details": {"reason": "Replayed timestamp"}}),
            ),
            AuthError::InvalidPubkey => (
                StatusCode::UNAUTHORIZED,
                json!({"error": "Unauthorized", "details": {"reason": "Invalid device pubkey"}}),
            ),
        };
        (status, axum::Json(body)).into_response()
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
        let claims = verify(&token, &state.decoding_key, "server")?;
        Ok(ServerAuth(claims.sub))
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

/// Bearer auth for the `device-cert` JWT type minted by the api/ Worker in
/// remote-hash-server mode. Carries the device's Ed25519 pubkey (embedded in
/// the JWT's `pubkey` claim, never persisted server-side) so the handler can
/// verify a per-request signature via `verify_signature`.
pub struct DeviceCertAuth(pub String, pub VerifyingKey);

impl FromRequestParts<Arc<AppState>> for DeviceCertAuth {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_token(parts)?;
        let claims = verify(&token, &state.decoding_key, "device-cert")?;

        let pubkey_b64 = claims.pubkey.ok_or(AuthError::InvalidPubkey)?;
        let pubkey_bytes = STANDARD
            .decode(pubkey_b64)
            .map_err(|_| AuthError::InvalidPubkey)?;
        let pubkey_arr: [u8; 32] = pubkey_bytes
            .try_into()
            .map_err(|_| AuthError::InvalidPubkey)?;
        let verifying_key =
            VerifyingKey::from_bytes(&pubkey_arr).map_err(|_| AuthError::InvalidPubkey)?;

        Ok(DeviceCertAuth(claims.sub, verifying_key))
    }
}

/// Verifies the Ed25519 signature attached to a device-cert-authenticated
/// request and enforces replay protection.
///
/// Signed message layout:
/// `timestamp_ms (i64 LE, 8 bytes) || device_id || 0x00 || method || 0x00 || path || 0x00 || body`
///
/// Headers: `X-Signature-Timestamp` (decimal ms) and `X-Signature` (base64).
///
/// Freshness is checked first (`|now - timestamp| <= 60s`), then the
/// signature itself, and only after a *successful* signature check is the
/// per-device replay-guard watermark inspected and advanced — checking
/// replay before the signature would let a forged signature burn a device's
/// timestamp watermark and DoS its next legitimate request.
pub fn verify_signature(
    headers: &HeaderMap,
    device_id: &str,
    pubkey: &VerifyingKey,
    method: &str,
    path: &str,
    body: &[u8],
    replay_guard: &DashMap<String, i64>,
) -> Result<(), AuthError> {
    let timestamp_ms: i64 = headers
        .get("x-signature-timestamp")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .ok_or(AuthError::InvalidSignature)?;

    let sig_b64 = headers
        .get("x-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or(AuthError::InvalidSignature)?;

    let now_ms = crate::db::now_ms();
    if (now_ms - timestamp_ms).abs() > 60_000 {
        return Err(AuthError::StaleTimestamp);
    }

    let sig_bytes = STANDARD
        .decode(sig_b64)
        .map_err(|_| AuthError::InvalidSignature)?;
    let sig_arr: [u8; 64] = sig_bytes.try_into().map_err(|_| AuthError::InvalidSignature)?;
    let signature = Signature::from_bytes(&sig_arr);

    let mut msg = Vec::with_capacity(8 + device_id.len() + 1 + method.len() + 1 + path.len() + 1 + body.len());
    msg.extend_from_slice(&timestamp_ms.to_le_bytes());
    msg.extend_from_slice(device_id.as_bytes());
    msg.push(0);
    msg.extend_from_slice(method.as_bytes());
    msg.push(0);
    msg.extend_from_slice(path.as_bytes());
    msg.push(0);
    msg.extend_from_slice(body);

    pubkey
        .verify_strict(&msg, &signature)
        .map_err(|_| AuthError::InvalidSignature)?;

    match replay_guard.entry(device_id.to_owned()) {
        Entry::Occupied(mut entry) => {
            if timestamp_ms <= *entry.get() {
                return Err(AuthError::ReplayedTimestamp);
            }
            entry.insert(timestamp_ms);
        }
        Entry::Vacant(entry) => {
            entry.insert(timestamp_ms);
        }
    }

    Ok(())
}
