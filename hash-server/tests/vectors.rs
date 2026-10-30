use std::time::Duration;

use axum::ServiceExt;
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
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
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

fn vector_cases(file: &str) -> Vec<Value> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../batch-format/vectors")
        .join(file);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let value: Value = serde_json::from_str(&text).unwrap();
    value["cases"].as_array().unwrap().clone()
}

async fn ingest_all(
    server: &TestServer,
    client: &reqwest::Client,
    device_id: &str,
    content_hashes: &[Vec<u8>],
    expected_states: &[String],
    case: &str,
) {
    let token = server.token(device_id, "device");
    let server_token = server.token("ignored", "server");
    for (i, hash) in content_hashes.iter().enumerate() {
        let resp = client
            .post(format!("{}/hash", server.base_url))
            .header("Authorization", format!("Bearer {token}"))
            .body(post_body(
                1000 + i as u32,
                i as u32 + 1,
                hash.as_slice().try_into().unwrap(),
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201, "{case}: ingest {i}");

        let resp = client
            .get(format!("{}/hash?devices={device_id}", server.base_url))
            .header("Authorization", format!("Bearer {server_token}"))
            .send()
            .await
            .unwrap();
        let body: Value = resp.json().await.unwrap();
        assert_eq!(
            body[device_id]["hash"], expected_states[i],
            "{case}: state after {i}"
        );
    }
}

/// BATCH-006, HASH-006: the server's stored state follows
/// `state = sha256(state || content_hash)` from 32 zero bytes, and the
/// resulting states match the shared vectors that the TypeScript verifier
/// (`verifyBatch`) and the Rust client are also checked against.
#[tokio::test]
async fn rolling_state_matches_shared_vectors() {
    let server = spawn_server().await;
    let client = reqwest::Client::new();
    let mut checked = 0;
    for (n, case) in vector_cases("rolling-hash.json").iter().enumerate() {
        // The server always starts from zero.
        if case["start_state_hex"] != hex::encode([0u8; 32]) {
            continue;
        }
        let name = case["name"].as_str().unwrap();
        let hashes: Vec<Vec<u8>> = case["content_hashes_hex"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| hex::decode(h.as_str().unwrap()).unwrap())
            .collect();
        let states: Vec<String> = case["states_hex"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h.as_str().unwrap().to_string())
            .collect();
        let device_id = format!("aaaaaaaa-0000-4000-8000-{n:012}");
        ingest_all(&server, &client, &device_id, &hashes, &states, name).await;

        // HASH-008, BATCH-006: after the API resets the device (the batch
        // upload), the next batch chains from zero again and reaches the same
        // end state.
        let server_token = server.token("ignored", "server");
        let resp = client
            .delete(format!("{}/hash?device={device_id}", server.base_url))
            .header("Authorization", format!("Bearer {server_token}"))
            .send()
            .await
            .unwrap();
        let prior: Value = resp.json().await.unwrap();
        assert_eq!(
            prior["hash"], case["end_state_hex"],
            "{name}: prior state on reset"
        );
        assert_eq!(prior["seq"], hashes.len() as u64, "{name}: prior seq");
        ingest_all(&server, &client, &device_id, &hashes, &states, name).await;
        checked += 1;
    }
    assert!(
        checked >= 8,
        "expected the zero-start rolling cases, got {checked}"
    );
}

/// BATCH-006: the server chains whatever 32-byte content hashes the client
/// sends, so the content hashes of the event vectors reach the same state
/// as the vectors' `state_from_zero_hex`.
#[tokio::test]
async fn event_content_hashes_chain_to_vector_state() {
    let server = spawn_server().await;
    let client = reqwest::Client::new();
    for (n, case) in vector_cases("event-hash.json").iter().enumerate() {
        let name = case["name"].as_str().unwrap();
        let hash = hex::decode(case["content_hash_hex"].as_str().unwrap()).unwrap();
        let expected = vec![case["state_from_zero_hex"].as_str().unwrap().to_string()];
        let device_id = format!("bbbbbbbb-0000-4000-8000-{n:012}");
        ingest_all(&server, &client, &device_id, &[hash], &expected, name).await;
    }
}
