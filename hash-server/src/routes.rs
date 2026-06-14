use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bytes::Bytes;
use serde_json::json;
use std::sync::Arc;

use crate::{
    AppState,
    auth::{DeviceAccessAuth, DeviceAccessOrServerAuth, ServerAuth},
    db,
};

#[derive(Debug, thiserror::Error)]
#[error("database error")]
pub struct DbError(#[from] sqlx::Error);

impl IntoResponse for DbError {
    fn into_response(self) -> Response {
        tracing::error!("db error: {}", self.0);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Internal Server Error"})),
        )
            .into_response()
    }
}

pub async fn health() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

pub async fn post_hash(
    DeviceAccessAuth(device_id): DeviceAccessAuth,
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> Result<impl IntoResponse, DbError> {
    if body.len() != 32 {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Bad Request", "details": {"body": ["Expected exactly 32 bytes"]}})),
        )
            .into_response());
    }

    let hash: [u8; 32] = body.as_ref().try_into().unwrap();
    db::update_hash_chain(&state.pool, &device_id, &hash).await?;

    Ok(Json(json!({"ok": true})).into_response())
}

pub async fn get_hash(
    DeviceAccessAuth(device_id): DeviceAccessAuth,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, DbError> {
    let bytes = db::get_hash_state(&state.pool, &device_id)
        .await?
        .unwrap_or([0u8; 32]);

    Ok((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        bytes.to_vec(),
    )
        .into_response())
}

pub async fn delete_hash(
    ServerAuth(device_id): ServerAuth,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, DbError> {
    db::reset_hash_state(&state.pool, &device_id).await?;
    Ok(Json(json!({"ok": true})))
}

pub async fn get_hash_info(
    DeviceAccessOrServerAuth(device_id): DeviceAccessOrServerAuth,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, DbError> {
    let info = db::get_hash_info(&state.pool, &device_id).await?;
    Ok(Json(json!({
        "count": info.as_ref().map_or(0, |i| i.count),
        "hashed_at": info.as_ref().and_then(|i| i.hashed_at),
        "updated_at": info.map(|i| i.updated_at),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use ed25519_dalek::{SigningKey, pkcs8::EncodePrivateKey};
    use http::{Method, Request, StatusCode};
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    use serde::Serialize;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::ServiceExt;

    async fn make_test_state() -> (Arc<AppState>, EncodingKey) {
        // Use a fixed seed so keys are deterministic across test runs.
        let signing_key = SigningKey::from_bytes(&[42u8; 32]);
        let verifying_key = signing_key.verifying_key();

        let pkcs8 = signing_key.to_pkcs8_der().unwrap();
        // DecodingKey::from_ed_der expects raw 32-byte key, not SPKI DER.
        let raw_pub = verifying_key.to_bytes();

        let enc_key = EncodingKey::from_ed_der(pkcs8.as_bytes());
        let dec_key = jsonwebtoken::DecodingKey::from_ed_der(&raw_pub);

        let pool = crate::db::tests::in_memory_pool().await;
        let state = Arc::new(AppState { pool, decoding_key: dec_key });
        (state, enc_key)
    }

    fn make_token(enc: &EncodingKey, typ: &str, sub: &str) -> String {
        #[derive(Serialize)]
        struct Claims<'a> {
            sub: &'a str,
            #[serde(rename = "type")]
            typ: &'a str,
            exp: u64,
        }
        let exp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 3600;
        encode(&Header::new(Algorithm::EdDSA), &Claims { sub, typ, exp }, enc).unwrap()
    }

    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // ── health ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn health_returns_ok() {
        let (state, _) = make_test_state().await;
        let app = crate::build_router(state);
        let req = Request::builder().uri("/health").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(json_body(resp).await["ok"], true);
    }

    // ── POST /hash ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn post_hash_valid() {
        let (state, enc) = make_test_state().await;
        let app = crate::build_router(state);
        let token = make_token(&enc, "device-access", "dev-1");
        let req = Request::builder()
            .method(Method::POST)
            .uri("/hash")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(vec![0xabu8; 32]))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(json_body(resp).await["ok"], true);
    }

    #[tokio::test]
    async fn post_hash_wrong_size_is_400() {
        let (state, enc) = make_test_state().await;
        let app = crate::build_router(state);
        let token = make_token(&enc, "device-access", "dev-1");
        let req = Request::builder()
            .method(Method::POST)
            .uri("/hash")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(vec![0u8; 16]))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn post_hash_no_auth_is_401() {
        let (state, _) = make_test_state().await;
        let app = crate::build_router(state);
        let req = Request::builder()
            .method(Method::POST)
            .uri("/hash")
            .body(Body::from(vec![0u8; 32]))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn post_hash_server_token_rejected() {
        let (state, enc) = make_test_state().await;
        let app = crate::build_router(state);
        let token = make_token(&enc, "server", "dev-1");
        let req = Request::builder()
            .method(Method::POST)
            .uri("/hash")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(vec![0u8; 32]))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = json_body(resp).await;
        assert_eq!(body["details"]["reason"], "Invalid token type");
    }

    // ── GET /hash ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn get_hash_no_state_returns_zeros() {
        let (state, enc) = make_test_state().await;
        let app = crate::build_router(state);
        let token = make_token(&enc, "device-access", "dev-1");
        let req = Request::builder()
            .uri("/hash")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&bytes[..], &[0u8; 32]);
    }

    #[tokio::test]
    async fn get_hash_reflects_post() {
        let (state, enc) = make_test_state().await;
        let app = crate::build_router(Arc::clone(&state));

        let token = make_token(&enc, "device-access", "dev-1");

        // POST a hash
        let post_req = Request::builder()
            .method(Method::POST)
            .uri("/hash")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(vec![0xffu8; 32]))
            .unwrap();
        app.clone().oneshot(post_req).await.unwrap();

        // GET should return non-zero state
        let get_req = Request::builder()
            .uri("/hash")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let resp = crate::build_router(state).oneshot(get_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(bytes.len(), 32);
        assert_ne!(&bytes[..], &[0u8; 32]);
    }

    // ── DELETE /hash ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn delete_hash_resets_state() {
        let (state, enc) = make_test_state().await;

        let device_token = make_token(&enc, "device-access", "dev-1");
        let server_token = make_token(&enc, "server", "dev-1");

        // POST to set some state
        let post_req = Request::builder()
            .method(Method::POST)
            .uri("/hash")
            .header("authorization", format!("Bearer {device_token}"))
            .body(Body::from(vec![0xffu8; 32]))
            .unwrap();
        crate::build_router(Arc::clone(&state)).oneshot(post_req).await.unwrap();

        // DELETE with server token
        let del_req = Request::builder()
            .method(Method::DELETE)
            .uri("/hash")
            .header("authorization", format!("Bearer {server_token}"))
            .body(Body::empty())
            .unwrap();
        let resp = crate::build_router(Arc::clone(&state)).oneshot(del_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // GET should now return zeros
        let get_req = Request::builder()
            .uri("/hash")
            .header("authorization", format!("Bearer {device_token}"))
            .body(Body::empty())
            .unwrap();
        let resp = crate::build_router(state).oneshot(get_req).await.unwrap();
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&bytes[..], &[0u8; 32]);
    }

    #[tokio::test]
    async fn delete_hash_device_token_rejected() {
        let (state, enc) = make_test_state().await;
        let app = crate::build_router(state);
        let token = make_token(&enc, "device-access", "dev-1");
        let req = Request::builder()
            .method(Method::DELETE)
            .uri("/hash")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
