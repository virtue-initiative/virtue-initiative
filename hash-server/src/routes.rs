use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bytes::Bytes;
use serde_json::json;
use std::sync::Arc;

use crate::{
    AppState,
    auth::{self, AuthError, DeviceAccessOrServerAuth, DeviceCertAuth, ServerAuth},
    db,
    writer::WriteError,
};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("database error")]
    Db(#[from] sqlx::Error),
    #[error("server is overloaded")]
    QueueFull,
    #[error("writer unavailable")]
    WriterGone,
    #[error(transparent)]
    Auth(#[from] AuthError),
}

impl From<WriteError> for ApiError {
    fn from(e: WriteError) -> Self {
        match e {
            WriteError::QueueFull => ApiError::QueueFull,
            WriteError::WriterGone => ApiError::WriterGone,
            WriteError::Db(e) => {
                tracing::error!("db error: {e}");
                ApiError::WriterGone
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::Db(e) => {
                tracing::error!("db error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "Internal Server Error"})),
                )
                    .into_response()
            }
            ApiError::WriterGone => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Internal Server Error"})),
            )
                .into_response(),
            ApiError::QueueFull => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "Service Unavailable", "details": {"reason": "server is overloaded, please retry"}})),
            )
                .into_response(),
            ApiError::Auth(e) => e.into_response(),
        }
    }
}

pub async fn health() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

/// The only per-device, high-frequency, TLS-handshake-sensitive endpoint —
/// it's the one that moved off TLS onto plain HTTP, so it's also the only
/// one that needs the device-cert + Ed25519 signature scheme in place of
/// what TLS used to provide (authenticity + integrity per request).
pub async fn post_hash(
    DeviceCertAuth(device_id, pubkey): DeviceCertAuth,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if body.len() != 32 {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Bad Request", "details": {"body": ["Expected exactly 32 bytes"]}})),
        )
            .into_response());
    }

    auth::verify_signature(
        &headers,
        &device_id,
        &pubkey,
        "POST",
        "/hash",
        &body,
        &state.replay_guard,
    )?;

    let hash: [u8; 32] = body.as_ref().try_into().unwrap();
    state.write.update_hash_chain(&device_id, hash).await?;

    Ok(Json(json!({"ok": true})).into_response())
}

/// Server-only (see module docs on post_hash): merges the former
/// GET /hash/info into GET /hash. Still bearer-JWT-only, unsigned — this
/// caller (api/'s Worker) never runs over the TLS-handshake-cost path this
/// plan's device-cert work exists to avoid.
pub async fn get_hash(
    DeviceAccessOrServerAuth(device_id): DeviceAccessOrServerAuth,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, ApiError> {
    let state_bytes = db::get_hash_state(&state.read_pool, &device_id)
        .await?
        .unwrap_or([0u8; 32]);
    let info = db::get_hash_info(&state.read_pool, &device_id).await?;

    Ok(Json(json!({
        "state": STANDARD.encode(state_bytes),
        "count": info.as_ref().map_or(0, |i| i.count),
        "hashed_at": info.as_ref().and_then(|i| i.hashed_at),
        "updated_at": info.map(|i| i.updated_at),
    })))
}

pub async fn delete_hash(
    ServerAuth(device_id): ServerAuth,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, ApiError> {
    state.write.reset_hash_state(&device_id).await?;
    Ok(Json(json!({"ok": true})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use ed25519_dalek::{Signer, SigningKey, pkcs8::EncodePrivateKey};
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

        // Shared-cache in-memory DB so the write connection and the read
        // pool see the same data, mirroring production's shared file.
        use sqlx::Connection;
        use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection, SqlitePoolOptions};
        use std::str::FromStr;

        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .shared_cache(true);

        let mut write_conn = SqliteConnection::connect_with(&opts).await.unwrap();
        sqlx::migrate!("./migrations").run(&mut write_conn).await.unwrap();
        let write = crate::writer::spawn(write_conn, 1024);

        let read_pool = SqlitePoolOptions::new()
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();

        let state = Arc::new(AppState {
            read_pool,
            write,
            decoding_key: dec_key,
            replay_guard: dashmap::DashMap::new(),
        });
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

    /// Mints a `device-cert`-typed token embedding the given device's raw
    /// Ed25519 pubkey (base64), mirroring what api/'s buildDeviceState mints
    /// in remote-hash-server mode.
    fn make_device_cert_token(enc: &EncodingKey, sub: &str, pubkey: &[u8; 32]) -> String {
        #[derive(Serialize)]
        struct Claims<'a> {
            sub: &'a str,
            #[serde(rename = "type")]
            typ: &'a str,
            pubkey: &'a str,
            exp: u64,
        }
        let exp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 3600;
        let pubkey_b64 = STANDARD.encode(pubkey);
        encode(
            &Header::new(Algorithm::EdDSA),
            &Claims { sub, typ: "device-cert", pubkey: &pubkey_b64, exp },
            enc,
        )
        .unwrap()
    }

    /// Signs a request at an explicit timestamp, mirroring the byte layout
    /// `auth::verify_signature` reconstructs server-side. Returns the
    /// base64 signature only — callers that don't need to tamper with the
    /// timestamp should use `sign_request` instead.
    fn sign_request_at(
        signing_key: &SigningKey,
        timestamp_ms: i64,
        device_id: &str,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> String {
        let mut msg = Vec::new();
        msg.extend_from_slice(&timestamp_ms.to_le_bytes());
        msg.extend_from_slice(device_id.as_bytes());
        msg.push(0);
        msg.extend_from_slice(method.as_bytes());
        msg.push(0);
        msg.extend_from_slice(path.as_bytes());
        msg.push(0);
        msg.extend_from_slice(body);
        let sig = signing_key.sign(&msg);
        STANDARD.encode(sig.to_bytes())
    }

    /// Signs a request at the current time. Returns `(timestamp_ms, base64_sig)`.
    fn sign_request(
        signing_key: &SigningKey,
        device_id: &str,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> (i64, String) {
        let timestamp_ms = crate::db::now_ms();
        let sig = sign_request_at(signing_key, timestamp_ms, device_id, method, path, body);
        (timestamp_ms, sig)
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
    async fn post_hash_valid_signed_request_accepted() {
        let (state, enc) = make_test_state().await;
        let app = crate::build_router(state);

        let device_signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let pubkey = device_signing_key.verifying_key().to_bytes();
        let cert_token = make_device_cert_token(&enc, "dev-1", &pubkey);

        let body = vec![0xabu8; 32];
        let (timestamp_ms, sig) = sign_request(&device_signing_key, "dev-1", "POST", "/hash", &body);

        let req = Request::builder()
            .method(Method::POST)
            .uri("/hash")
            .header("authorization", format!("Bearer {cert_token}"))
            .header("x-signature-timestamp", timestamp_ms.to_string())
            .header("x-signature", sig)
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(json_body(resp).await["ok"], true);
    }

    #[tokio::test]
    async fn post_hash_wrong_size_is_400() {
        let (state, enc) = make_test_state().await;
        let app = crate::build_router(state);

        let device_signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let pubkey = device_signing_key.verifying_key().to_bytes();
        let cert_token = make_device_cert_token(&enc, "dev-1", &pubkey);

        // Wrong-size body is rejected before signature verification even
        // runs, so an arbitrary (unsigned) timestamp/signature is fine here.
        let req = Request::builder()
            .method(Method::POST)
            .uri("/hash")
            .header("authorization", format!("Bearer {cert_token}"))
            .header("x-signature-timestamp", crate::db::now_ms().to_string())
            .header("x-signature", "not-checked")
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
    async fn post_hash_device_access_token_rejected() {
        // The old device-access-typed token (pre-device-cert scheme) must
        // no longer work on /hash now that it requires a device-cert token.
        let (state, enc) = make_test_state().await;
        let app = crate::build_router(state);
        let token = make_token(&enc, "device-access", "dev-1");
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

    #[tokio::test]
    async fn post_hash_stale_timestamp_rejected() {
        let (state, enc) = make_test_state().await;
        let app = crate::build_router(state);

        let device_signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let pubkey = device_signing_key.verifying_key().to_bytes();
        let cert_token = make_device_cert_token(&enc, "dev-1", &pubkey);

        let body = vec![0xabu8; 32];
        let stale_timestamp_ms = crate::db::now_ms() - 61_000;
        let sig = sign_request_at(&device_signing_key, stale_timestamp_ms, "dev-1", "POST", "/hash", &body);

        let req = Request::builder()
            .method(Method::POST)
            .uri("/hash")
            .header("authorization", format!("Bearer {cert_token}"))
            .header("x-signature-timestamp", stale_timestamp_ms.to_string())
            .header("x-signature", sig)
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = json_body(resp).await;
        assert_eq!(body["details"]["reason"], "Stale timestamp");
    }

    #[tokio::test]
    async fn post_hash_replayed_timestamp_rejected() {
        let (state, enc) = make_test_state().await;
        let app = crate::build_router(Arc::clone(&state));

        let device_signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let pubkey = device_signing_key.verifying_key().to_bytes();
        let cert_token = make_device_cert_token(&enc, "dev-1", &pubkey);

        let body = vec![0xabu8; 32];
        let (timestamp_ms, sig) = sign_request(&device_signing_key, "dev-1", "POST", "/hash", &body);

        let make_req = || {
            Request::builder()
                .method(Method::POST)
                .uri("/hash")
                .header("authorization", format!("Bearer {cert_token}"))
                .header("x-signature-timestamp", timestamp_ms.to_string())
                .header("x-signature", sig.clone())
                .body(Body::from(body.clone()))
                .unwrap()
        };

        let first = app.clone().oneshot(make_req()).await.unwrap();
        assert_eq!(first.status(), StatusCode::OK);

        // Identical (non-increasing) timestamp + signature replayed verbatim.
        let second = crate::build_router(state).oneshot(make_req()).await.unwrap();
        assert_eq!(second.status(), StatusCode::UNAUTHORIZED);
        let body = json_body(second).await;
        assert_eq!(body["details"]["reason"], "Replayed timestamp");
    }

    #[tokio::test]
    async fn post_hash_tampered_body_rejected() {
        let (state, enc) = make_test_state().await;
        let app = crate::build_router(state);

        let device_signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let pubkey = device_signing_key.verifying_key().to_bytes();
        let cert_token = make_device_cert_token(&enc, "dev-1", &pubkey);

        let signed_body = vec![0xabu8; 32];
        let (timestamp_ms, sig) =
            sign_request(&device_signing_key, "dev-1", "POST", "/hash", &signed_body);

        // Send a different (still 32-byte) body than what was signed.
        let sent_body = vec![0xcdu8; 32];
        let req = Request::builder()
            .method(Method::POST)
            .uri("/hash")
            .header("authorization", format!("Bearer {cert_token}"))
            .header("x-signature-timestamp", timestamp_ms.to_string())
            .header("x-signature", sig)
            .body(Body::from(sent_body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = json_body(resp).await;
        assert_eq!(body["details"]["reason"], "Invalid signature");
    }

    #[tokio::test]
    async fn post_hash_tampered_signature_rejected() {
        let (state, enc) = make_test_state().await;
        let app = crate::build_router(state);

        let device_signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let pubkey = device_signing_key.verifying_key().to_bytes();
        let cert_token = make_device_cert_token(&enc, "dev-1", &pubkey);

        let body = vec![0xabu8; 32];
        let (timestamp_ms, sig) = sign_request(&device_signing_key, "dev-1", "POST", "/hash", &body);

        // Flip one byte of the decoded signature before re-encoding.
        let mut sig_bytes = STANDARD.decode(&sig).unwrap();
        sig_bytes[0] ^= 0xff;
        let tampered_sig = STANDARD.encode(sig_bytes);

        let req = Request::builder()
            .method(Method::POST)
            .uri("/hash")
            .header("authorization", format!("Bearer {cert_token}"))
            .header("x-signature-timestamp", timestamp_ms.to_string())
            .header("x-signature", tampered_sig)
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = json_body(resp).await;
        assert_eq!(body["details"]["reason"], "Invalid signature");
    }

    // ── GET /hash (merged with the former GET /hash/info) ───────────────────────

    #[tokio::test]
    async fn get_hash_no_state_returns_zeroed_json() {
        let (state, enc) = make_test_state().await;
        let app = crate::build_router(state);
        let token = make_token(&enc, "server", "dev-1");
        let req = Request::builder()
            .uri("/hash")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["state"], STANDARD.encode([0u8; 32]));
        assert_eq!(body["count"], 0);
        assert_eq!(body["hashed_at"], serde_json::Value::Null);
        assert_eq!(body["updated_at"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn get_hash_reflects_post_and_reports_count() {
        let (state, enc) = make_test_state().await;
        let app = crate::build_router(Arc::clone(&state));

        let device_signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let pubkey = device_signing_key.verifying_key().to_bytes();
        let cert_token = make_device_cert_token(&enc, "dev-1", &pubkey);
        let server_token = make_token(&enc, "server", "dev-1");

        // POST a hash via the signed device-cert path.
        let body = vec![0xffu8; 32];
        let (timestamp_ms, sig) = sign_request(&device_signing_key, "dev-1", "POST", "/hash", &body);
        let post_req = Request::builder()
            .method(Method::POST)
            .uri("/hash")
            .header("authorization", format!("Bearer {cert_token}"))
            .header("x-signature-timestamp", timestamp_ms.to_string())
            .header("x-signature", sig)
            .body(Body::from(body))
            .unwrap();
        let post_resp = app.clone().oneshot(post_req).await.unwrap();
        assert_eq!(post_resp.status(), StatusCode::OK);

        // GET (server-only, unsigned) should reflect the merged JSON shape.
        let get_req = Request::builder()
            .uri("/hash")
            .header("authorization", format!("Bearer {server_token}"))
            .body(Body::empty())
            .unwrap();
        let resp = crate::build_router(state).oneshot(get_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_ne!(body["state"], STANDARD.encode([0u8; 32]));
        assert_eq!(body["count"], 1);
        assert!(body["hashed_at"].is_number());
        assert!(body["updated_at"].is_number());
    }

    #[tokio::test]
    async fn get_hash_device_access_token_still_accepted() {
        // get_hash keeps whatever DeviceAccessOrServerAuth-equivalent auth
        // the old get_hash_info accepted — unaffected by this plan.
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
    }

    // ── DELETE /hash (unaffected by this plan) ───────────────────────────────────

    #[tokio::test]
    async fn delete_hash_resets_state() {
        let (state, enc) = make_test_state().await;

        let device_signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let pubkey = device_signing_key.verifying_key().to_bytes();
        let cert_token = make_device_cert_token(&enc, "dev-1", &pubkey);
        let server_token = make_token(&enc, "server", "dev-1");

        // POST to set some state
        let body = vec![0xffu8; 32];
        let (timestamp_ms, sig) = sign_request(&device_signing_key, "dev-1", "POST", "/hash", &body);
        let post_req = Request::builder()
            .method(Method::POST)
            .uri("/hash")
            .header("authorization", format!("Bearer {cert_token}"))
            .header("x-signature-timestamp", timestamp_ms.to_string())
            .header("x-signature", sig)
            .body(Body::from(body))
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

        // GET should now report zeroed state again
        let get_req = Request::builder()
            .uri("/hash")
            .header("authorization", format!("Bearer {server_token}"))
            .body(Body::empty())
            .unwrap();
        let resp = crate::build_router(state).oneshot(get_req).await.unwrap();
        let body = json_body(resp).await;
        assert_eq!(body["state"], STANDARD.encode([0u8; 32]));
    }

    #[tokio::test]
    async fn delete_hash_device_access_token_rejected() {
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
