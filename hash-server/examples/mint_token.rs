//! Dev/test-only helper: signs a hash-server or server JWT from a private key
//! PEM file, for manual testing (curl) and `scripts/bench.sh`. The hash
//! server itself never mints tokens — that's the main API's job.

use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;

#[derive(Serialize)]
struct Claims {
    sub: String,
    #[serde(rename = "type")]
    typ: String,
    exp: usize,
}

fn main() {
    let mut args = env::args().skip(1);
    let usage = "usage: mint_token <sub> <hash-server|server> <private_key_pem_path>";
    let sub = args.next().expect(usage);
    let typ = args.next().expect(usage);
    let key_path = args.next().expect(usage);

    let pem = std::fs::read_to_string(&key_path)
        .unwrap_or_else(|e| panic!("failed to read {key_path}: {e}"));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let claims = Claims {
        sub,
        typ,
        exp: (now + 3600) as usize,
    };

    let key =
        EncodingKey::from_ed_pem(pem.as_bytes()).expect("private key must be an Ed25519 PKCS8 PEM");
    let token =
        encode(&Header::new(Algorithm::EdDSA), &claims, &key).expect("failed to sign token");

    println!("{token}");
}
