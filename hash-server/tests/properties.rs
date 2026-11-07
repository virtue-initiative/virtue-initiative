//! Property tests for the hash server's chain logic (`hash-server/SPEC.md`).
//!
//! The server runs for real on a local port and the tests only speak HTTP,
//! like `integration.rs`. Case counts are low because every case starts a
//! server with its own database.

use std::time::Duration;

use axum::ServiceExt;
use ed25519_dalek::SigningKey;
use hash_server::config::Config;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use pkcs8::{EncodePrivateKey, EncodePublicKey};
use proptest::prelude::*;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

struct TestServer {
    base_url: String,
    signing_key_pem: String,
}

#[derive(Serialize)]
struct Claims {
    sub: String,
    #[serde(rename = "type")]
    typ: String,
    exp: usize,
}

impl TestServer {
    fn token(&self, sub: &str, typ: &str) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let claims = Claims {
            sub: sub.to_string(),
            typ: typ.to_string(),
            exp: (now + 3600) as usize,
        };
        let key = EncodingKey::from_ed_pem(self.signing_key_pem.as_bytes()).unwrap();
        encode(&Header::new(Algorithm::EdDSA), &claims, &key).unwrap()
    }
}

async fn spawn_server() -> TestServer {
    let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
    let signing_key_pem = signing_key
        .to_pkcs8_pem(Default::default())
        .unwrap()
        .to_string();
    let public_key_pem = signing_key
        .verifying_key()
        .to_public_key_pem(Default::default())
        .unwrap();
    let db_path = std::env::temp_dir().join(format!(
        "hash-server-prop-{}-{}.sqlite",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let config = Config {
        bind_addr: "127.0.0.1:0".to_string(),
        database_path: db_path.to_string_lossy().to_string(),
        jwt_public_key_pem: public_key_pem,
        write_batch_window: Duration::from_millis(1),
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
    }
}

fn post_body(seq: u32, hash: [u8; 32]) -> Vec<u8> {
    let mut body = Vec::with_capacity(40);
    body.extend_from_slice(&1000u32.to_le_bytes());
    body.extend_from_slice(&seq.to_le_bytes());
    body.extend_from_slice(&hash);
    body
}

fn roll(state: [u8; 32], hash: [u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(state);
    hasher.update(hash);
    hasher.finalize().into()
}

fn run<F: std::future::Future<Output = ()>>(f: F) {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(f);
}

/// Sequence numbers drawn from a small range, so that repeats, decreases and
/// the invalid `0` all turn up often.
fn posts() -> impl Strategy<Value = Vec<(u32, [u8; 32])>> {
    prop::collection::vec((0u32..=12, any::<[u8; 32]>()), 1..12)
}

// HASH-006: the stored state is `sha256(stored || hash)` folded over every
// accepted hash, starting from 32 zero bytes. A post is accepted (201) only
// when its `seq` is strictly greater than the last accepted one, and a
// rejected post (409) leaves the state and the sequence number as they were.
proptest! {
    #![proptest_config(ProptestConfig { cases: 12, failure_persistence: None, ..ProptestConfig::default() })]

    // HASH-006: state is the fold of accepted hashes; seq must increase.
    #[test]
    fn stored_state_is_the_fold_of_the_accepted_hashes(posts in posts()) {
        run(async move {
            let server = spawn_server().await;
            let client = reqwest::Client::new();
            let device = "33333333-3333-4333-8333-333333333333";
            let device_token = server.token(device, "device");
            let server_token = server.token("ignored", "server");

            let mut state = [0u8; 32];
            let mut last_seq = 0u32;
            for (seq, hash) in posts {
                let resp = client
                    .post(format!("{}/hash", server.base_url))
                    .header("Authorization", format!("Bearer {device_token}"))
                    .body(post_body(seq, hash))
                    .send()
                    .await
                    .unwrap();
                if seq > last_seq {
                    assert_eq!(resp.status(), 201, "seq {seq} after {last_seq}");
                    state = roll(state, hash);
                    last_seq = seq;
                } else {
                    assert_eq!(resp.status(), 409, "seq {seq} after {last_seq}");
                }

                let body: Value = client
                    .get(format!("{}/hash?devices={device}", server.base_url))
                    .header("Authorization", format!("Bearer {server_token}"))
                    .send()
                    .await
                    .unwrap()
                    .json()
                    .await
                    .unwrap();
                assert_eq!(body[device]["hash"], hex::encode(state));
                assert_eq!(body[device]["seq"], last_seq);
            }
        });
    }
}

// HASH-006, HASH-007 and HASH-008: devices do not share state, and a DELETE returns the
// state that was stored and starts the device again from zero, where `seq`
// can start again from 1.
proptest! {
    #![proptest_config(ProptestConfig { cases: 8, failure_persistence: None, ..ProptestConfig::default() })]

    // HASH-006, HASH-007, HASH-008: devices are independent; DELETE resets.
    #[test]
    fn devices_are_independent_and_delete_resets_to_zero(
        first in prop::collection::vec(any::<[u8; 32]>(), 1..6),
        second in prop::collection::vec(any::<[u8; 32]>(), 1..6),
    ) {
        run(async move {
            let server = spawn_server().await;
            let client = reqwest::Client::new();
            let a = "44444444-4444-4444-8444-444444444444";
            let b = "55555555-5555-4555-8555-555555555555";
            let server_token = server.token("ignored", "server");

            for (device, hashes) in [(a, &first), (b, &second)] {
                let token = server.token(device, "device");
                for (i, hash) in hashes.iter().enumerate() {
                    let resp = client
                        .post(format!("{}/hash", server.base_url))
                        .header("Authorization", format!("Bearer {token}"))
                        .body(post_body(i as u32 + 1, *hash))
                        .send()
                        .await
                        .unwrap();
                    assert_eq!(resp.status(), 201);
                }
            }

            let fold = |hashes: &Vec<[u8; 32]>| hashes.iter().fold([0u8; 32], |s, h| roll(s, *h));

            let deleted: Value = client
                .delete(format!("{}/hash?device={a}", server.base_url))
                .header("Authorization", format!("Bearer {server_token}"))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(deleted["hash"], hex::encode(fold(&first)));

            let body: Value = client
                .get(format!("{}/hash?devices={a},{b}", server.base_url))
                .header("Authorization", format!("Bearer {server_token}"))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(body[a]["hash"], hex::encode([0u8; 32]));
            assert_eq!(body[a]["seq"], 0);
            assert_eq!(body[b]["hash"], hex::encode(fold(&second)));

            let resp = client
                .post(format!("{}/hash", server.base_url))
                .header("Authorization", format!("Bearer {}", server.token(a, "device")))
                .body(post_body(1, first[0]))
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status(), 201);
        });
    }
}
