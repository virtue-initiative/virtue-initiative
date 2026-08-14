use axum::http::HeaderMap;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::Deserialize;

use crate::error::ApiError;

#[derive(Debug, Deserialize)]
pub struct Claims {
    pub sub: String,
    #[serde(rename = "type")]
    pub typ: String,
}

pub struct JwtVerifier {
    decoding_key: DecodingKey,
    validation: Validation,
}

impl JwtVerifier {
    pub fn new(public_key_pem: &str) -> Self {
        let decoding_key = DecodingKey::from_ed_pem(public_key_pem.as_bytes())
            .expect("JWT_PUBLIC_KEY must be a valid Ed25519 SPKI PEM public key");

        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.validate_aud = false;
        validation.required_spec_claims.clear();

        JwtVerifier {
            decoding_key,
            validation,
        }
    }

    /// Verifies the bearer token in `headers` and checks its `type` claim is
    /// exactly `expected_type`. Any failure (missing header, bad signature,
    /// expired token, wrong type) is reported as 401 per SPEC.md.
    pub fn require(&self, headers: &HeaderMap, expected_type: &str) -> Result<Claims, ApiError> {
        let token = bearer_token(headers)?;

        let claims = decode::<Claims>(token, &self.decoding_key, &self.validation)
            .map_err(|_| ApiError::Unauthorized(None))?
            .claims;

        if claims.typ != expected_type {
            return Err(ApiError::Unauthorized(Some(format!(
                "expected token type '{expected_type}'"
            ))));
        }

        Ok(claims)
    }
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, ApiError> {
    let header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError::Unauthorized(None))?;

    header
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty())
        .ok_or(ApiError::Unauthorized(None))
}
