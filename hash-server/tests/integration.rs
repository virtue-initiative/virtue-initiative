use std::time::Duration;

use ed25519_dalek::SigningKey;
use hash_server::config::Config;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey};
use serde::Serialize;
use serde_json::Value;

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
        let claims = Claims {
            sub: sub.to_string(),
            typ: typ.to_string(),
            exp: (chrono_now() + 3600) as usize,
        };
        let key = EncodingKey::from_ed_pem(self.signing_key_pem.as_bytes()).unwrap();
        encode(&Header::new(Algorithm::EdDSA), &claims, &key).unwrap()
    }
}

fn chrono_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

async fn spawn_server() -> TestServer {
    let mut csprng = rand::rngs::OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let signing_key_pem = signing_key
        .to_pkcs8_pem(Default::default())
        .unwrap()
        .to_string();
    let public_key_pem = signing_key
        .verifying_key()
        .to_public_key_pem(Default::default())
        .unwrap();
    // Round-trip through the private key to make sure our PEM matches what
    // jsonwebtoken/openssl would produce, since `Default::default()` line
    // endings must be LF for from_ed_pem to accept it.
    let _ = SigningKey::from_pkcs8_pem(&signing_key_pem).unwrap();

    let db_path = std::env::temp_dir().join(format!(
        "hash-server-test-{}-{}.sqlite",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));

    let config = Config {
        bind_addr: "127.0.0.1:0".to_string(),
        database_path: db_path.to_string_lossy().to_string(),
        jwt_public_key_pem: public_key_pem,
        write_batch_window: Duration::from_millis(2),
    };

    let state = hash_server::init(&config);
    let app = hash_server::router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    TestServer {
        base_url: format!("http://{addr}"),
        signing_key_pem,
    }
}

fn post_body(unix_time: u32, seq: u32, hash: [u8; 32]) -> Vec<u8> {
    let mut body = Vec::with_capacity(40);
    body.extend_from_slice(&unix_time.to_le_bytes());
    body.extend_from_slice(&seq.to_le_bytes());
    body.extend_from_slice(&hash);
    body
}

#[tokio::test]
async fn get_root_returns_status_without_auth() {
    let server = spawn_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "Virtue Initiative Hash API");
    assert_eq!(body["status"], "ok");
    assert!(body["version"].is_string());
    assert!(body["commit"].is_string());
}

#[tokio::test]
async fn post_hash_requires_valid_jwt() {
    let server = spawn_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/hash", server.base_url))
        .body(post_body(1000, 1, [1u8; 32]))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    let resp = client
        .post(format!("{}/hash", server.base_url))
        .header("Authorization", "Bearer not-a-real-token")
        .body(post_body(1000, 1, [1u8; 32]))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // A `server`-typed token must not authorize POST /hash.
    let wrong_type_token = server.token("device-1", "server");
    let resp = client
        .post(format!("{}/hash", server.base_url))
        .header("Authorization", format!("Bearer {wrong_type_token}"))
        .body(post_body(1000, 1, [1u8; 32]))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn post_hash_rejects_bad_body_length() {
    let server = spawn_server().await;
    let client = reqwest::Client::new();
    let token = server.token("11111111-1111-4111-8111-111111111111", "device");

    let resp = client
        .post(format!("{}/hash", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .body(vec![0u8; 10])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "invalid_body");
}

#[tokio::test]
async fn post_hash_chains_and_enforces_strictly_increasing_seq() {
    let server = spawn_server().await;
    let client = reqwest::Client::new();
    let device_id = "22222222-2222-4222-8222-222222222222";
    let token = server.token(device_id, "device");
    let server_token = server.token("ignored", "server");

    let resp = client
        .post(format!("{}/hash", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .body(post_body(1000, 1, [1u8; 32]))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // seq must be strictly greater than the last one.
    let resp = client
        .post(format!("{}/hash", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .body(post_body(1001, 1, [2u8; 32]))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "sequence_conflict");

    let resp = client
        .post(format!("{}/hash", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .body(post_body(1002, 2, [2u8; 32]))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let expected_first = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update([0u8; 32]);
        hasher.update([1u8; 32]);
        let stage1: [u8; 32] = hasher.finalize().into();

        let mut hasher = Sha256::new();
        hasher.update(stage1);
        hasher.update([2u8; 32]);
        let stage2: [u8; 32] = hasher.finalize().into();
        hex::encode(stage2)
    };

    let resp = client
        .get(format!("{}/hash?devices={device_id}", server.base_url))
        .header("Authorization", format!("Bearer {server_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body[device_id]["hash"], expected_first);
    assert_eq!(body[device_id]["seq"], 2);
    assert_eq!(body[device_id]["last_received"], 1002);
}

#[tokio::test]
async fn get_hash_returns_zero_state_for_unknown_devices() {
    let server = spawn_server().await;
    let client = reqwest::Client::new();
    let server_token = server.token("ignored", "server");
    let unknown = "33333333-3333-4333-8333-333333333333";

    let resp = client
        .get(format!("{}/hash?devices={unknown}", server.base_url))
        .header("Authorization", format!("Bearer {server_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body[unknown]["hash"], hex::encode([0u8; 32]));
    assert_eq!(body[unknown]["seq"], 0);
    assert_eq!(body[unknown]["last_received"], 0);
}

#[tokio::test]
async fn get_hash_rejects_malformed_device_ids() {
    let server = spawn_server().await;
    let client = reqwest::Client::new();
    let server_token = server.token("ignored", "server");

    let resp = client
        .get(format!("{}/hash?devices=not-a-uuid", server.base_url))
        .header("Authorization", format!("Bearer {server_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn delete_hash_resets_and_returns_prior_state() {
    let server = spawn_server().await;
    let client = reqwest::Client::new();
    let device_id = "44444444-4444-4444-8444-444444444444";
    let token = server.token(device_id, "device");
    let server_token = server.token("ignored", "server");

    client
        .post(format!("{}/hash", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .body(post_body(500, 5, [9u8; 32]))
        .send()
        .await
        .unwrap();

    let resp = client
        .delete(format!("{}/hash?device={device_id}", server.base_url))
        .header("Authorization", format!("Bearer {server_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["seq"], 5);
    assert_eq!(body["last_received"], 500);

    let resp = client
        .delete(format!("{}/hash?device={device_id}", server.base_url))
        .header("Authorization", format!("Bearer {server_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["hash"], hex::encode([0u8; 32]));
    assert_eq!(body["seq"], 0);
    // SPEC.md section 2.3: a reset "SHOULD NOT reset the last_received time" —
    // this reflects state going into the *second* reset, i.e. right after the
    // first one, so last_received must still be what the first POST set.
    assert_eq!(body["last_received"], 500);

    // Sequence numbers reset to zero, so the next write must use seq > 0.
    let resp = client
        .post(format!("{}/hash", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .body(post_body(600, 0, [1u8; 32]))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);

    let resp = client
        .post(format!("{}/hash", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .body(post_body(600, 1, [1u8; 32]))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
}

#[tokio::test]
async fn delete_hash_rejects_device_typed_token() {
    let server = spawn_server().await;
    let client = reqwest::Client::new();
    let device_id = "55555555-5555-4555-8555-555555555555";
    // DELETE requires a `server`-typed token (sub ignored); a `device`-typed
    // token, even for the same device, must not authorize a reset.
    let token = server.token(device_id, "device");

    let resp = client
        .delete(format!("{}/hash?device={device_id}", server.base_url))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}
