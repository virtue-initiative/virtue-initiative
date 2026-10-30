//! Helpers shared by the HTTP-level test files: a hash server on a free port, tokens
//! signed for it, and request builders.
#![allow(dead_code)]

use std::time::Duration;

use axum::ServiceExt;
use ed25519_dalek::SigningKey;
use hash_server::config::Config;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use pkcs8::{EncodePrivateKey, EncodePublicKey};
use reqwest::Response;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const DEVICE_A: &str = "aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa";
pub const DEVICE_B: &str = "bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb";
pub const ZERO_HEX: &str = "0000000000000000000000000000000000000000000000000000000000000000";

pub struct TestServer {
    pub base_url: String,
    pub signing_key_pem: String,
    pub client: reqwest::Client,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn new_signing_key_pem() -> (String, String) {
    let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
    let private = signing_key
        .to_pkcs8_pem(Default::default())
        .unwrap()
        .to_string();
    let public = signing_key
        .verifying_key()
        .to_public_key_pem(Default::default())
        .unwrap();
    (private, public)
}

pub fn sign_eddsa(claims: &Value, private_pem: &str) -> String {
    let key = EncodingKey::from_ed_pem(private_pem.as_bytes()).unwrap();
    encode(&Header::new(Algorithm::EdDSA), claims, &key).unwrap()
}

impl TestServer {
    /// A correctly signed token with the given `sub` and `type`, valid for an hour.
    pub fn token(&self, sub: &str, typ: &str) -> String {
        self.token_with(json!({ "sub": sub, "type": typ, "exp": now() + 3600 }))
    }

    /// A token signed with the server's key carrying exactly these claims.
    pub fn token_with(&self, claims: Value) -> String {
        sign_eddsa(&claims, &self.signing_key_pem)
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    pub async fn post_raw(&self, token: &str, body: Vec<u8>) -> Response {
        self.client
            .post(self.url("/hash"))
            .header("Authorization", format!("Bearer {token}"))
            .body(body)
            .send()
            .await
            .unwrap()
    }

    /// POST /hash as `device` with a fresh device token.
    pub async fn post(&self, device: &str, unix_time: u32, seq: u32, hash: [u8; 32]) -> Response {
        let token = self.token(device, "device");
        self.post_raw(&token, post_body(unix_time, seq, hash)).await
    }

    pub async fn get_devices(&self, query: &str) -> Response {
        let token = self.token("ignored", "server");
        self.client
            .get(self.url(&format!("/hash?{query}")))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap()
    }

    pub async fn delete_device(&self, query: &str) -> Response {
        let token = self.token("ignored", "server");
        self.client
            .delete(self.url(&format!("/hash?{query}")))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap()
    }

    /// The `{hash, seq, last_received}` entry that GET /hash reports for a device.
    pub async fn state(&self, device: &str) -> Value {
        let resp = self.get_devices(&format!("devices={device}")).await;
        assert_eq!(resp.status(), 200);
        let mut body: Value = resp.json().await.unwrap();
        body[device].take()
    }
}

pub async fn spawn_server() -> TestServer {
    let (signing_key_pem, public_key_pem) = new_signing_key_pem();
    let db_path = std::env::temp_dir().join(format!(
        "hash-server-edge-{}-{}.sqlite",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let config = Config {
        bind_addr: "127.0.0.1:0".to_string(),
        database_path: db_path.to_string_lossy().to_string(),
        jwt_public_key_pem: public_key_pem,
        write_batch_window: Duration::from_millis(2),
    };
    let app = hash_server::router(hash_server::init(&config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    TestServer {
        base_url: format!("http://{addr}"),
        signing_key_pem,
        client: reqwest::Client::new(),
    }
}

pub fn post_body(unix_time: u32, seq: u32, hash: [u8; 32]) -> Vec<u8> {
    let mut body = Vec::with_capacity(40);
    body.extend_from_slice(&unix_time.to_le_bytes());
    body.extend_from_slice(&seq.to_le_bytes());
    body.extend_from_slice(&hash);
    body
}

pub fn chain(state: [u8; 32], hash: [u8; 32]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(state);
    hasher.update(hash);
    hex::encode(hasher.finalize())
}

/// HASH-002: every error body is JSON with a known `code`, a string `message`,
/// and an optional string `details`.
pub async fn assert_error_shape(resp: Response, status: u16, code: &str) {
    assert_eq!(resp.status(), status);
    let body: Value = resp
        .json()
        .await
        .unwrap_or_else(|e| panic!("{status} {code}: body is not JSON: {e}"));
    let known = [
        "invalid_body",
        "invalid_query",
        "unauthorized",
        "forbidden",
        "not_found",
        "method_not_allowed",
        "gone",
        "sequence_conflict",
        "internal_error",
    ];
    assert!(
        known.contains(&body["code"].as_str().unwrap_or("")),
        "{body}"
    );
    assert_eq!(body["code"], code, "{body}");
    assert!(body["message"].as_str().is_some_and(|m| !m.is_empty()));
    assert!(body["details"].is_null() || body["details"].is_string());
    let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
    assert!(
        keys.iter()
            .all(|k| ["code", "message", "details"].contains(&k.as_str())),
        "unexpected keys in {body}"
    );
}
