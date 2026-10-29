//! Golden test vectors for the cross-language contracts in `batch-format/SPEC.md`.
//!
//! The JSON files in `batch-format/vectors/` are shared with the TypeScript
//! tests in `web/src/utils/api/vectors.test.ts` (and, for the hash chain, with
//! `hash-server/tests/vectors.rs`). Every case is checked against the
//! production Rust functions here. The ignored `generate_vectors` test writes
//! the files using those same functions.
//!
//! Hex is lowercase. Bytes inside event JSON are written as `{"$hex": "..."}`.

use std::io::{Read, Write};
use std::path::PathBuf;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use hpke::aead::AesGcm256;
use hpke::kdf::HkdfSha256;
use hpke::kem::{Kem as KemTrait, X25519HkdfSha256};
use hpke::{Deserializable, OpModeR, Serializable, setup_receiver};
use serde_bytes::{ByteBuf, Bytes};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use virtue_core::crypto::{
    CryptoEngine, compute_event_hash, derive_password_auth, encode_batch_event,
};
use virtue_core::model::{BatchRecipient, HashParams, LogEntry};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn vectors_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../batch-format/vectors")
}

fn load(name: &str) -> Value {
    let path = vectors_dir().join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name} is not valid JSON: {e}"))
}

fn cases(file: &Value) -> &Vec<Value> {
    file["cases"].as_array().expect("cases array")
}

fn hex_enc(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_dec(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd-length hex: {s}");
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("hex digit"))
        .collect()
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key]
        .as_str()
        .unwrap_or_else(|| panic!("missing string field {key} in {v}"))
}

fn hex_list(v: &Value, key: &str) -> Vec<Vec<u8>> {
    v[key]
        .as_array()
        .unwrap_or_else(|| panic!("missing array field {key}"))
        .iter()
        .map(|x| hex_dec(x.as_str().expect("hex string")))
        .collect()
}

/// Replaces every `{"$hex": "..."}` with the JSON array of byte values, the
/// form `LogEntry` deserializes `serde_bytes` fields from.
fn expand_hex(v: &Value) -> Value {
    match v {
        Value::Object(map) if map.len() == 1 && map.contains_key("$hex") => {
            let bytes = hex_dec(map["$hex"].as_str().expect("$hex string"));
            Value::Array(bytes.into_iter().map(|b| json!(b)).collect())
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, x)| (k.clone(), expand_hex(x)))
                .collect::<Map<_, _>>(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(expand_hex).collect()),
        other => other.clone(),
    }
}

fn log_entry(event: &Value) -> LogEntry {
    serde_json::from_value(expand_hex(event)).expect("vector event is a valid LogEntry")
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

/// Test-side reference of the BATCH-006 chain (core itself only sends content
/// hashes; the rolling state lives in the hash server).
fn chain(start: [u8; 32], content_hashes: &[[u8; 32]]) -> Vec<[u8; 32]> {
    let mut state = start;
    content_hashes
        .iter()
        .map(|h| {
            state = sha256(&[&state, h]);
            state
        })
        .collect()
}

fn private_key(bytes: &[u8]) -> <X25519HkdfSha256 as KemTrait>::PrivateKey {
    <<X25519HkdfSha256 as KemTrait>::PrivateKey as Deserializable>::from_bytes(bytes)
        .expect("32-byte X25519 private key")
}

fn public_key_of(private_key_bytes: &[u8]) -> Vec<u8> {
    let sk = private_key(private_key_bytes);
    X25519HkdfSha256::sk_to_pk(&sk).to_bytes().to_vec()
}

/// Opens `enc || ciphertext` per BATCH-005 (base mode, empty info and aad).
fn open_envelope(private_key_bytes: &[u8], envelope: &[u8]) -> Result<Vec<u8>, String> {
    if envelope.len() < 32 {
        return Err("envelope shorter than enc".into());
    }
    let sk = private_key(private_key_bytes);
    let enc = <<X25519HkdfSha256 as KemTrait>::EncappedKey as Deserializable>::from_bytes(
        &envelope[..32],
    )
    .map_err(|e| e.to_string())?;
    let mut ctx =
        setup_receiver::<AesGcm256, HkdfSha256, X25519HkdfSha256>(&OpModeR::Base, &sk, &enc, b"")
            .map_err(|e| e.to_string())?;
    ctx.open(&envelope[32..], b"").map_err(|e| e.to_string())
}

/// The BATCH-002 payload: msgpack array of `bin` values.
fn payload_of(encoded_events: &[Vec<u8>]) -> Vec<u8> {
    let items: Vec<&Bytes> = encoded_events.iter().map(|e| Bytes::new(e)).collect();
    rmp_serde::to_vec_named(&items).expect("payload")
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("gzip");
    encoder.finish().expect("gzip finish")
}

fn gunzip(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    GzDecoder::new(data).read_to_end(&mut out).expect("gunzip");
    out
}

/// Consumer side of BATCH-002: blob -> encoded events.
fn open_blob(batch_key: &[u8], blob: &[u8]) -> Vec<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(batch_key).expect("32-byte key");
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&blob[..12]), &blob[12..])
        .expect("blob decrypts with the batch key");
    let payload = gunzip(&plaintext);
    let events: Vec<ByteBuf> = rmp_serde::from_slice(&payload).expect("payload is a msgpack array");
    events.into_iter().map(|e| e.into_vec()).collect()
}

fn hash_params(v: &Value) -> HashParams {
    HashParams {
        version: "test".to_string(),
        algorithm: "argon2id".to_string(),
        memory_cost_kib: v["memory_cost_kib"].as_u64().unwrap() as u32,
        time_cost: v["time_cost"].as_u64().unwrap() as u32,
        parallelism: v["parallelism"].as_u64().unwrap() as u32,
        salt_length: 16,
        hkdf_hash: "sha256".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------

// BATCH-003, BATCH-004: an event encodes to exactly the vector's msgpack bytes,
// and those bytes decode back to the same event.
#[test]
fn event_encoding_matches_vectors() {
    let file = load("event-hash.json");
    let mut checked = 0;
    for case in cases(&file) {
        let name = s(case, "name");
        if case.get("event").is_none() {
            continue;
        }
        let expected = hex_dec(s(case, "encoded_hex"));
        let entry = log_entry(&case["event"]);
        let encoded = encode_batch_event(&entry).expect("encode");
        assert_eq!(hex_enc(&encoded), hex_enc(&expected), "case {name}");

        let decoded: LogEntry = rmp_serde::from_slice(&expected).expect("decode vector bytes");
        let reencoded = encode_batch_event(&decoded).expect("re-encode");
        assert_eq!(hex_enc(&reencoded), hex_enc(&expected), "round trip {name}");
        checked += 1;
    }
    assert!(checked >= 10, "expected many event cases, got {checked}");
}

// BATCH-006: content hash is sha256 of the encoded event bytes, as received.
#[test]
fn event_content_hash_matches_vectors() {
    let file = load("event-hash.json");
    for case in cases(&file) {
        let name = s(case, "name");
        let bytes = hex_dec(s(case, "encoded_hex"));
        let expected = s(case, "content_hash_hex");
        assert_eq!(
            hex_enc(&compute_event_hash(&bytes)),
            expected,
            "case {name}"
        );
        let first_state = hex_enc(&sha256(&[&[0u8; 32], &hex_dec(expected)]));
        assert_eq!(s(case, "state_from_zero_hex"), first_state, "case {name}");
    }
}

// BATCH-006: rolling state vectors are self-consistent with core's content
// hash. (The server-side computation is checked in hash-server/tests/vectors.rs.)
#[test]
fn rolling_hash_vectors_are_consistent_with_content_hash() {
    let file = load("rolling-hash.json");
    for case in cases(&file) {
        let name = s(case, "name");
        let start: [u8; 32] = hex_dec(s(case, "start_state_hex")).try_into().unwrap();
        let content_hashes: Vec<[u8; 32]> = hex_list(case, "content_hashes_hex")
            .into_iter()
            .map(|h| h.try_into().unwrap())
            .collect();
        if let Some(events) = case.get("events_hex") {
            let derived: Vec<String> = events
                .as_array()
                .unwrap()
                .iter()
                .map(|e| hex_enc(&compute_event_hash(&hex_dec(e.as_str().unwrap()))))
                .collect();
            let expected: Vec<String> = content_hashes.iter().map(|h| hex_enc(h)).collect();
            assert_eq!(derived, expected, "content hashes of {name}");
        }
        let states: Vec<String> = chain(start, &content_hashes)
            .iter()
            .map(|h| hex_enc(h))
            .collect();
        let expected: Vec<String> = case["states_hex"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        assert_eq!(states, expected, "states of {name}");
        let end = states.last().cloned().unwrap_or_else(|| hex_enc(&start));
        assert_eq!(s(case, "end_state_hex"), end, "end state of {name}");
    }
}

// BATCH-002, BATCH-004, BATCH-005, BATCH-006: consume every batch blob the
// vectors hold, whichever implementation produced it.
#[test]
fn batch_blobs_decode_and_verify() {
    let file = load("batch.json");
    for case in cases(&file) {
        let name = s(case, "name");
        let sk = hex_dec(s(case, "recipient_private_key_hex"));
        assert_eq!(
            B64.encode(public_key_of(&sk)),
            s(case, "recipient_public_key_base64"),
            "public key of {name}"
        );

        // BATCH-005: the wrapped key opens to the batch key.
        let envelope = B64.decode(s(case, "access_key_base64")).expect("base64");
        assert_eq!(envelope.len(), 32 + 32 + 16, "envelope length of {name}");
        let batch_key = open_envelope(&sk, &envelope).expect("access key opens");
        assert_eq!(
            hex_enc(&batch_key),
            s(case, "batch_key_hex"),
            "key of {name}"
        );

        // BATCH-002/004: the blob holds exactly the vector's encoded events.
        let blob = hex_dec(s(case, "blob_hex"));
        let events = open_blob(&batch_key, &blob);
        let expected = hex_list(case, "encoded_events_hex");
        assert_eq!(events, expected, "events of {name}");

        // BATCH-006: hashes and end state.
        let hashes: Vec<[u8; 32]> = events.iter().map(|e| compute_event_hash(e)).collect();
        assert_eq!(
            hashes.iter().map(|h| hex_enc(h)).collect::<Vec<_>>(),
            hex_list(case, "content_hashes_hex")
                .iter()
                .map(|h| hex_enc(h))
                .collect::<Vec<_>>(),
            "content hashes of {name}"
        );
        let end = chain([0u8; 32], &hashes)
            .last()
            .copied()
            .unwrap_or([0u8; 32]);
        assert_eq!(hex_enc(&end), s(case, "end_hash_hex"), "end hash of {name}");

        // Canonically encoded events are what the Rust producer emits.
        if case["encoding"] == "canonical" {
            for (event, bytes) in case["events"].as_array().unwrap().iter().zip(&events) {
                let encoded = encode_batch_event(&log_entry(event)).expect("encode");
                assert_eq!(&encoded, bytes, "producer bytes of {name}");
            }
        }
    }
}

// BATCH-005: Rust opens every envelope in the vectors (made by Rust or by the
// TypeScript `encryptForPublicKey`) and rejects the tampered ones.
#[test]
fn hpke_envelopes_open_as_expected() {
    let file = load("hpke.json");
    for case in cases(&file) {
        let name = s(case, "name");
        let sk = hex_dec(s(case, "recipient_private_key_hex"));
        assert_eq!(
            hex_enc(&public_key_of(&sk)),
            s(case, "recipient_public_key_hex"),
            "public key of {name}"
        );
        let envelope = hex_dec(s(case, "envelope_hex"));
        let result = open_envelope(&sk, &envelope);
        if case["expect"] == "ok" {
            assert_eq!(
                hex_enc(&result.unwrap_or_else(|e| panic!("{name}: {e}"))),
                s(case, "plaintext_hex"),
                "plaintext of {name}"
            );
        } else {
            assert!(result.is_err(), "{name} must be rejected");
        }
    }
}

// BATCH-005: the production wrap function emits `enc || ciphertext` that a
// receiver built from the vector's private key opens (fresh ephemeral key).
#[test]
fn production_wrap_opens_with_vector_private_keys() {
    let file = load("hpke.json");
    let engine = CryptoEngine;
    for case in cases(&file).iter().filter(|c| c["expect"] == "ok") {
        let name = s(case, "name");
        let plaintext = hex_dec(s(case, "plaintext_hex"));
        let Ok(batch_key) = <[u8; 32]>::try_from(plaintext.as_slice()) else {
            continue;
        };
        let recipient = BatchRecipient {
            user_id: "vector".to_string(),
            pub_key_base64: B64.encode(hex_dec(s(case, "recipient_public_key_hex"))),
        };
        let wrapped = engine
            .wrap_batch_key_for_recipient(&recipient, &batch_key)
            .expect("wrap");
        let envelope = B64.decode(wrapped).unwrap();
        let sk = hex_dec(s(case, "recipient_private_key_hex"));
        assert_eq!(open_envelope(&sk, &envelope).unwrap(), plaintext, "{name}");
    }
}

// API-015, API-016: password_auth = HKDF-SHA256("auth", argon2id(password, salt)).
// `password_auth` goes over the wire as standard Base64 (API-010, API-016).
#[test]
fn password_auth_matches_vectors() {
    let file = load("password-auth.json");
    for case in cases(&file) {
        let name = s(case, "name");
        let salt = hex_dec(s(case, "salt_hex"));
        let params = hash_params(&case["params"]);
        let auth = derive_password_auth(s(case, "password"), &salt, &params).expect("derive");
        assert_eq!(hex_enc(&auth), s(case, "password_auth_hex"), "case {name}");
        assert_eq!(
            B64.encode(auth),
            s(case, "password_auth_base64"),
            "case {name}"
        );
    }
}

// ---------------------------------------------------------------------------
// Generator
// ---------------------------------------------------------------------------

fn bytes_json(bytes: &[u8]) -> Value {
    json!({ "$hex": hex_enc(bytes) })
}

fn seq_bytes(range: std::ops::RangeInclusive<u8>) -> Vec<u8> {
    range.collect()
}

/// Named events used for the event-hash vectors and to build batches.
fn named_events() -> Vec<(&'static str, Value)> {
    let all_byte_values = seq_bytes(0..=255);
    let mut v: Vec<(&'static str, Value)> = vec![
        (
            "heartbeat_no_risk_no_data",
            json!({"ts": 1700000000000_i64, "type": "heartbeat"}),
        ),
        ("ts_zero", json!({"ts": 0, "type": "user_start"})),
        (
            "ts_negative_one",
            json!({"ts": -1, "risk": 1.0, "type": "user_stop"}),
        ),
        (
            "ts_max_safe_integer",
            json!({"ts": 9007199254740991_i64, "type": "capture_failed"}),
        ),
        (
            "ts_above_u32",
            json!({"ts": 4294967296_i64, "type": "screenshot_missed", "risk": 0.0}),
        ),
        (
            "risk_inexact_in_f32",
            json!({"ts": 1700000001000_i64, "risk": 0.7, "type": "alert", "data": {"message": "risk 0.7"}}),
        ),
        (
            "risk_exact_in_f32",
            json!({"ts": 1700000002000_i64, "risk": 0.875, "type": "user_stop"}),
        ),
        (
            "screenshot_skipped_static",
            json!({"ts": 1700000003000_i64, "risk": 0.0, "type": "screenshot_skipped", "data": {"reason": "static_screen"}}),
        ),
        (
            "screenshot_skipped_locked",
            json!({"ts": 1700000004000_i64, "type": "screenshot_skipped", "data": {"reason": "locked_or_screensaver"}}),
        ),
        (
            "screenshot_with_scores",
            json!({"ts": 1700000005000_i64, "risk": 0.5, "type": "screenshot", "data": {
                "image": bytes_json(&seq_bytes(0..=63)),
                "content_type": "image/webp",
                "skin_detection": 0.25,
                "nsfw_detection": 0.75,
            }}),
        ),
        (
            "screenshot_empty_image",
            json!({"ts": 1700000006000_i64, "type": "screenshot", "data": {
                "image": bytes_json(&[]),
                "content_type": "image/png",
            }}),
        ),
        (
            "screenshot_image_255_bytes_bin8_max",
            json!({"ts": 1700000007000_i64, "type": "screenshot", "data": {
                "image": bytes_json(&seq_bytes(1..=255)),
                "content_type": "image/webp",
            }}),
        ),
        (
            "screenshot_image_256_bytes_bin16_all_byte_values",
            json!({"ts": 1700000008000_i64, "type": "screenshot", "data": {
                "image": bytes_json(&all_byte_values),
                "content_type": "image/webp",
            }}),
        ),
        (
            "system_login",
            json!({"ts": 1700000009000_i64, "type": "system_login", "data": {"utc_ms": 1699999990000_i64}}),
        ),
        (
            "system_logout",
            json!({"ts": 1700000010000_i64, "type": "system_logout", "data": {"utc_ms": 0}}),
        ),
        (
            "repeated_restarts",
            json!({"ts": 1700000011000_i64, "risk": 0.5, "type": "repeated_restarts", "data": {"count": 3, "window_ms": 600000}}),
        ),
        (
            "alert_unicode",
            json!({"ts": 1700000012000_i64, "risk": 1.0, "type": "alert", "data": {"message": "Tamper alert: na\u{ef}ve caf\u{e9} \u{65e5}\u{672c}\u{8a9e} \u{1f980}"}}),
        ),
        (
            "alert_empty_message",
            json!({"ts": 1700000013000_i64, "type": "alert", "data": {"message": ""}}),
        ),
        (
            "alert_300_chars_str16",
            json!({"ts": 1700000014000_i64, "type": "alert", "data": {"message": "a".repeat(300)}}),
        ),
        (
            "dev_details_nil",
            json!({"ts": 1700000015000_i64, "type": "dev", "data": {"title": "t", "details": null}}),
        ),
        (
            "dev_details_special_characters",
            json!({"ts": 1700000016000_i64, "type": "dev", "data": {
                "title": "Za\u{17c}\u{f3}\u{142}\u{107} g\u{119}\u{15b}l\u{105} ja\u{17a}\u{144}",
                "details": "line1\nline2\ttab \u{0} nul \"quoted\" back\\slash",
            }}),
        ),
    ];
    v.push((
        "user_stop_plain",
        json!({"ts": 1700000017000_i64, "type": "user_stop"}),
    ));
    v.push((
        "user_start_plain",
        json!({"ts": 1700000018000_i64, "type": "user_start"}),
    ));
    v
}

fn engine_event_bytes(event: &Value) -> Vec<u8> {
    encode_batch_event(&log_entry(event)).expect("encode")
}

const KEY_A: [u8; 32] = {
    let mut k = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        k[i] = (i as u8) + 1;
        i += 1;
    }
    k
};
const KEY_B: [u8; 32] = {
    let mut k = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        k[i] = 0xa0 ^ (i as u8).wrapping_mul(7);
        i += 1;
    }
    k
};

fn recipient_for(sk: &[u8]) -> BatchRecipient {
    BatchRecipient {
        user_id: "vector-recipient".to_string(),
        pub_key_base64: B64.encode(public_key_of(sk)),
    }
}

/// Builds one batch case with the production crypto functions. `encoded` are
/// the event bytes exactly as the payload should carry them.
fn batch_case(
    name: &str,
    note: &str,
    encoding: &str,
    events: Vec<Value>,
    encoded: Vec<Vec<u8>>,
    payload: Vec<u8>,
    sk: &[u8; 32],
) -> Value {
    let engine = CryptoEngine;
    let batch_key = engine.generate_batch_key();
    let blob = engine
        .encrypt_batch_blob(&batch_key, &gzip(&payload))
        .expect("encrypt");
    let recipient = recipient_for(sk);
    let access_key = engine
        .wrap_batch_key_for_recipient(&recipient, &batch_key)
        .expect("wrap");
    let hashes: Vec<[u8; 32]> = encoded.iter().map(|e| compute_event_hash(e)).collect();
    let end = chain([0u8; 32], &hashes)
        .last()
        .copied()
        .unwrap_or([0u8; 32]);
    json!({
        "name": name,
        "note": note,
        "producer": "rust",
        "encoding": encoding,
        "events": events,
        "encoded_events_hex": encoded.iter().map(|e| hex_enc(e)).collect::<Vec<_>>(),
        "content_hashes_hex": hashes.iter().map(|h| hex_enc(h)).collect::<Vec<_>>(),
        "end_hash_hex": hex_enc(&end),
        "recipient_private_key_hex": hex_enc(sk),
        "recipient_public_key_base64": recipient.pub_key_base64,
        "access_key_base64": access_key,
        "batch_key_hex": hex_enc(&batch_key),
        "blob_hex": hex_enc(&blob),
    })
}

fn canonical_batch(name: &str, note: &str, events: Vec<Value>, sk: &[u8; 32]) -> Value {
    let encoded: Vec<Vec<u8>> = events.iter().map(engine_event_bytes).collect();
    let payload = payload_of(&encoded);
    batch_case(name, note, "canonical", events, encoded, payload, sk)
}

#[allow(clippy::vec_init_then_push)]
fn batch_cases() -> Vec<Value> {
    let named = named_events();
    let by_name = |n: &str| -> Value {
        named
            .iter()
            .find(|(k, _)| *k == n)
            .unwrap_or_else(|| panic!("no event {n}"))
            .1
            .clone()
    };
    let mut out = Vec::new();

    out.push(canonical_batch(
        "single_event_no_risk_no_data",
        "The smallest batch the producer may build (BATCH-002).",
        vec![by_name("heartbeat_no_risk_no_data")],
        &KEY_A,
    ));

    // The producer rejects an empty batch (BATCH-002) but the consumer must
    // still verify one: the end hash of zero events is the zero state.
    out.push(batch_case(
        "empty_event_list",
        "Hand-assembled: BatchBuilder refuses to build this, a consumer must still accept it. The end hash is the 32 zero bytes.",
        "canonical",
        vec![],
        vec![],
        payload_of(&[]),
        &KEY_B,
    ));

    out.push(canonical_batch(
        "every_known_event_type",
        "One event of each known type, with and without risk (BATCH-003).",
        named.iter().map(|(_, e)| e.clone()).collect(),
        &KEY_A,
    ));

    out.push(canonical_batch(
        "binary_data_boundaries",
        "Images of 0, 255 and 256 bytes cross the bin8/bin16 boundary, and the outer array elements exceed 255 bytes (BATCH-004).",
        vec![
            by_name("screenshot_empty_image"),
            by_name("screenshot_image_255_bytes_bin8_max"),
            by_name("screenshot_image_256_bytes_bin16_all_byte_values"),
        ],
        &KEY_B,
    ));

    out.push(canonical_batch(
        "unicode_strings",
        "Multi-byte UTF-8 in titles, messages and details, plus an embedded NUL.",
        vec![
            by_name("alert_unicode"),
            by_name("dev_details_special_characters"),
            by_name("alert_300_chars_str16"),
        ],
        &KEY_A,
    ));

    // 200 events is the producer limit (BATCH-002, BATCH-006).
    let many: Vec<Value> = (0..200)
        .map(|i| {
            let mut e = json!({"ts": 1700000000000_i64 + i * 1000, "type": "heartbeat"});
            if i % 3 == 0 {
                e["risk"] = json!(0.25);
            }
            e
        })
        .collect();
    out.push(canonical_batch(
        "two_hundred_event_chain",
        "The producer's per-batch limit of 200 events (BATCH-002).",
        many,
        &KEY_A,
    ));

    // BATCH-004: batches written before the bin requirement hold arrays of ints.
    let legacy_events = vec![
        json!({"ts": 1700000000000_i64, "type": "heartbeat"}),
        json!({"ts": 1700000001000_i64, "risk": 0.5, "type": "screenshot", "data": {
            "image": bytes_json(&[0, 127, 128, 255]),
            "content_type": "image/webp",
        }}),
    ];
    let legacy_encoded: Vec<Vec<u8>> = legacy_events
        .iter()
        .map(|e| {
            // serde_json Value turns byte arrays into msgpack arrays of ints.
            let plain = expand_hex(e);
            rmp_serde::to_vec_named(&plain).expect("legacy event")
        })
        .collect();
    let legacy_payload = rmp_serde::to_vec_named(&legacy_encoded).expect("legacy payload");
    out.push(batch_case(
        "legacy_int_array_form",
        "Event elements and the screenshot image are msgpack arrays of integers, not bin. A consumer MUST accept this (BATCH-004). Keys are sorted, so the bytes differ from the canonical encoding.",
        "legacy_int_arrays",
        legacy_events,
        legacy_encoded,
        legacy_payload,
        &KEY_A,
    ));

    // BATCH-003: unknown types and keys must be ignored, not rejected.
    let future = json!({
        "ts": 1700000000000_i64,
        "risk": 0.5,
        "type": "future_event",
        "data": {"new_field": "value", "nested": {"a": [1, 2, 3]}},
        "extra_top_level_key": true
    });
    let future_encoded = vec![rmp_serde::to_vec_named(&future).expect("future event")];
    let future_payload = payload_of(&future_encoded);
    out.push(batch_case(
        "unknown_type_and_extra_keys",
        "An event type and keys this version does not know (BATCH-003). Rust cannot represent it as a LogEntry.",
        "future_type",
        vec![future],
        future_encoded,
        future_payload,
        &KEY_B,
    ));

    out
}

fn event_hash_cases() -> Vec<Value> {
    let mut out: Vec<Value> = named_events()
        .into_iter()
        .map(|(name, event)| {
            let encoded = engine_event_bytes(&event);
            let hash = compute_event_hash(&encoded);
            json!({
                "name": name,
                "event": event,
                "encoded_hex": hex_enc(&encoded),
                "content_hash_hex": hex_enc(&hash),
                "state_from_zero_hex": hex_enc(&sha256(&[&[0u8; 32], &hash])),
            })
        })
        .collect();
    // Raw byte strings: the hash covers whatever bytes arrive, whether or not
    // they are a valid event (BATCH-006).
    for (name, bytes) in [
        ("raw_empty_input", Vec::new()),
        ("raw_abc", b"abc".to_vec()),
        ("raw_non_msgpack_bytes", vec![0xff, 0x00, 0xc1, 0x80]),
        ("raw_1000_zero_bytes", vec![0u8; 1000]),
    ] {
        let hash = compute_event_hash(&bytes);
        out.push(json!({
            "name": name,
            "encoded_hex": hex_enc(&bytes),
            "content_hash_hex": hex_enc(&hash),
            "state_from_zero_hex": hex_enc(&sha256(&[&[0u8; 32], &hash])),
        }));
    }
    out
}

fn rolling_case(
    name: &str,
    start: [u8; 32],
    events: Option<Vec<Vec<u8>>>,
    content_hashes: Vec<[u8; 32]>,
) -> Value {
    let states = chain(start, &content_hashes);
    let mut case = json!({
        "name": name,
        "start_state_hex": hex_enc(&start),
        "content_hashes_hex": content_hashes.iter().map(|h| hex_enc(h)).collect::<Vec<_>>(),
        "states_hex": states.iter().map(|h| hex_enc(h)).collect::<Vec<_>>(),
        "end_state_hex": hex_enc(&states.last().copied().unwrap_or(start)),
    });
    if let Some(events) = events {
        case["events_hex"] = json!(events.iter().map(|e| hex_enc(e)).collect::<Vec<_>>());
    }
    case
}

fn rolling_cases() -> Vec<Value> {
    let zero = [0u8; 32];
    let named = named_events();
    let encoded: Vec<Vec<u8>> = named.iter().map(|(_, e)| engine_event_bytes(e)).collect();
    let hashes =
        |e: &[Vec<u8>]| -> Vec<[u8; 32]> { e.iter().map(|b| compute_event_hash(b)).collect() };

    let mut out = vec![
        rolling_case("empty_chain_stays_zero", zero, Some(vec![]), vec![]),
        rolling_case(
            "single_event",
            zero,
            Some(encoded[..1].to_vec()),
            hashes(&encoded[..1]),
        ),
        rolling_case(
            "three_events",
            zero,
            Some(encoded[5..8].to_vec()),
            hashes(&encoded[5..8]),
        ),
        rolling_case(
            "every_known_event_type",
            zero,
            Some(encoded.clone()),
            hashes(&encoded),
        ),
        rolling_case("all_zero_content_hash", zero, None, vec![[0u8; 32]]),
        rolling_case(
            "all_ff_content_hashes",
            zero,
            None,
            vec![[0xff; 32], [0xff; 32]],
        ),
        // Order matters: the same two events in the other order give another state.
        rolling_case(
            "order_a_b",
            zero,
            Some(vec![encoded[0].clone(), encoded[1].clone()]),
            vec![
                compute_event_hash(&encoded[0]),
                compute_event_hash(&encoded[1]),
            ],
        ),
        rolling_case(
            "order_b_a",
            zero,
            Some(vec![encoded[1].clone(), encoded[0].clone()]),
            vec![
                compute_event_hash(&encoded[1]),
                compute_event_hash(&encoded[0]),
            ],
        ),
        rolling_case(
            "non_zero_start_state",
            [0x5a; 32],
            Some(encoded[2..3].to_vec()),
            hashes(&encoded[2..3]),
        ),
    ];
    let two_hundred: Vec<Vec<u8>> = (0u32..200).map(|i| i.to_be_bytes().to_vec()).collect();
    out.push(rolling_case(
        "two_hundred_events",
        zero,
        Some(two_hundred.clone()),
        hashes(&two_hundred),
    ));
    out
}

fn hpke_cases(existing_other: Vec<Value>) -> Vec<Value> {
    let engine = CryptoEngine;
    let mut out = Vec::new();
    let sequential: Vec<u8> = (0u8..32).collect();
    let plaintexts: [(&str, Vec<u8>); 3] = [
        ("all_zero_batch_key", vec![0u8; 32]),
        ("all_ff_batch_key", vec![0xff; 32]),
        ("sequential_batch_key", sequential),
    ];
    for (i, (name, plaintext)) in plaintexts.iter().enumerate() {
        let sk: &[u8; 32] = if i % 2 == 0 { &KEY_A } else { &KEY_B };
        let recipient = recipient_for(sk);
        let key: [u8; 32] = plaintext.as_slice().try_into().unwrap();
        let wrapped = engine
            .wrap_batch_key_for_recipient(&recipient, &key)
            .expect("wrap");
        let envelope = B64.decode(wrapped).unwrap();
        out.push(json!({
            "name": name,
            "producer": "rust",
            "expect": "ok",
            "recipient_private_key_hex": hex_enc(sk),
            "recipient_public_key_hex": hex_enc(&public_key_of(sk)),
            "plaintext_hex": hex_enc(plaintext),
            "envelope_hex": hex_enc(&envelope),
        }));
    }

    // Failure cases are derived from the first Rust-produced envelope.
    let base = out[0].clone();
    let mut tampered = hex_dec(s(&base, "envelope_hex"));
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    let mut tampered_enc = hex_dec(s(&base, "envelope_hex"));
    tampered_enc[0] ^= 0x01;
    let truncated = hex_dec(s(&base, "envelope_hex"))[..40].to_vec();
    for (name, envelope, sk) in [
        ("tampered_ciphertext_tag", tampered, &KEY_A),
        ("tampered_encapsulated_key", tampered_enc, &KEY_A),
        ("truncated_envelope", truncated, &KEY_A),
        (
            "wrong_recipient_key",
            hex_dec(s(&base, "envelope_hex")),
            &KEY_B,
        ),
    ] {
        out.push(json!({
            "name": name,
            "producer": "rust",
            "expect": "fail",
            "recipient_private_key_hex": hex_enc(sk),
            "recipient_public_key_hex": hex_enc(&public_key_of(sk)),
            "envelope_hex": hex_enc(&envelope),
        }));
    }
    out.extend(existing_other);
    out
}

/// (name, password, salt, (memory KiB, time, parallelism), note)
type PasswordDef = (
    &'static str,
    &'static str,
    Vec<u8>,
    (u32, u32, u32),
    &'static str,
);

fn password_cases() -> Vec<Value> {
    let salt16: Vec<u8> = (0u8..16).map(|i| i.wrapping_mul(17) ^ 0x3c).collect();
    let defs: Vec<PasswordDef> = vec![
        (
            "ascii_password_email_salt_cost_65536_3",
            "correct horse battery staple",
            b"user@example.com".to_vec(),
            (65536, 3, 1),
            "The cost the repo CLAUDE.md names. The salt is an opaque byte string (API-015); here it happens to be a lowercase email.",
        ),
        (
            "non_ascii_password_cost_65536_3",
            "p\u{e4}ssw\u{f6}rd-\u{65e5}\u{672c}\u{8a9e}-\u{1f980}",
            salt16.clone(),
            (65536, 3, 1),
            "Non-ASCII password: both sides hash its UTF-8 bytes with no normalization.",
        ),
        (
            "current_api_params_131072_5",
            "hunter2",
            salt16.clone(),
            (131072, 5, 1),
            "HashParams currently served by the API (api/src/lib/password.ts).",
        ),
        (
            "uppercase_email_salt_is_not_lowercased",
            "correct horse battery staple",
            b"User@Example.COM".to_vec(),
            (1024, 2, 1),
            "The salt is used as given. Neither side lowercases it, so this differs from the lowercase-email case at equal cost.",
        ),
        (
            "empty_password_low_cost",
            "",
            salt16.clone(),
            (64, 1, 1),
            "Empty password.",
        ),
        (
            "parallelism_2_low_cost",
            "correct horse battery staple",
            salt16.clone(),
            (256, 2, 2),
            "parallelism other than 1 reaches argon2id.",
        ),
        (
            "low_cost_ascii",
            "testpassword",
            b"user@example.com".to_vec(),
            (64, 1, 1),
            "Cheap parameters, for quick regression.",
        ),
    ];
    defs.into_iter()
        .map(|(name, password, salt, (m, t, p), note)| {
            let params = HashParams {
                version: "test".to_string(),
                algorithm: "argon2id".to_string(),
                memory_cost_kib: m,
                time_cost: t,
                parallelism: p,
                salt_length: 16,
                hkdf_hash: "sha256".to_string(),
            };
            let auth = derive_password_auth(password, &salt, &params).expect("derive");
            json!({
                "name": name,
                "note": note,
                "password": password,
                "salt_hex": hex_enc(&salt),
                "params": {"memory_cost_kib": m, "time_cost": t, "parallelism": p},
                "password_auth_hex": hex_enc(&auth),
                "password_auth_base64": B64.encode(auth),
                "slow": m >= 65536,
            })
        })
        .collect()
}

/// Cases from other producers (the TypeScript generator), kept across regeneration.
fn preserved_cases(file: &str) -> Vec<Value> {
    let path = vectors_dir().join(file);
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    value["cases"]
        .as_array()
        .map(|cases| {
            cases
                .iter()
                .filter(|c| c["producer"].as_str().is_some_and(|p| p != "rust"))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn write_vector(file: &str, description: &str, cases: Vec<Value>) {
    let dir = vectors_dir();
    std::fs::create_dir_all(&dir).expect("create vectors dir");
    let value = json!({ "description": description, "cases": cases });
    let mut text = serde_json::to_string_pretty(&value).expect("serialize");
    text.push('\n');
    std::fs::write(dir.join(file), text).expect("write vector file");
}

// BATCH-008: regenerates the canonical vectors from the production functions.
#[test]
#[ignore = "generator: run with --ignored to regenerate batch-format/vectors"]
fn generate_vectors() {
    write_vector(
        "event-hash.json",
        "Event encoding and content hash (BATCH-003, BATCH-004, BATCH-006). `event` is the logical event; bytes inside it are {\"$hex\": ...}. `encoded_hex` is the msgpack the Rust producer emits for it (lowercase hex). `content_hash_hex` = sha256(encoded bytes). `state_from_zero_hex` = sha256(32 zero bytes || content hash), the chain state after this one event. Cases without `event` are raw byte strings. Floats in `risk`, `skin_detection` and `nsfw_detection` are stored as msgpack float32; a consumer sees the float32 value widened to a double.",
        event_hash_cases(),
    );
    write_vector(
        "rolling-hash.json",
        "Hash chain (BATCH-006, HASH-006). state = sha256(state || content_hash), starting from `start_state_hex`. `states_hex[i]` is the state after the first i+1 hashes. `events_hex`, when present, are encoded events whose sha256 are `content_hashes_hex`. All values are lowercase hex. Cases with a non-zero start state apply to consumers that take a start hash, not to the hash server, which always starts from zero.",
        rolling_cases(),
    );
    write_vector(
        "batch.json",
        "Encrypted batches (BATCH-002 to BATCH-006). Each case fixes a batch blob and how to open it. `producer` says which implementation made the bytes (`rust` or `ts-seed`). `recipient_private_key_hex` opens `access_key_base64` (standard Base64 of enc || ciphertext) to `batch_key_hex`, which decrypts `blob_hex` (lowercase hex of nonce[12] || ciphertext+tag) to gzip(msgpack([bin, ...])). The array elements are `encoded_events_hex`; `content_hashes_hex` and `end_hash_hex` follow BATCH-006 from a zero start. `events` is the logical content after decoding (bytes as {\"$hex\": ...}). `encoding` is `canonical` (the Rust producer's exact bytes), `legacy_int_arrays` (BATCH-004 legacy form) or `future_type` (BATCH-003 unknown type).",
        {
            let mut cases = batch_cases();
            cases.extend(preserved_cases("batch.json"));
            cases
        },
    );
    write_vector(
        "hpke.json",
        "HPKE key wrapping (BATCH-005): DHKEM(X25519, HKDF-SHA256) / HKDF-SHA256 / AES-256-GCM, base mode, empty info and aad. `envelope_hex` is enc[32] || ciphertext (plaintext length + 16 tag bytes), lowercase hex. `recipient_private_key_hex` is the raw 32-byte X25519 scalar and `recipient_public_key_hex` its public key. `producer` is `rust` or `ts`. With `expect` = `ok` the envelope opens to `plaintext_hex`, with `fail` it MUST be rejected.",
        hpke_cases(preserved_cases("hpke.json")),
    );
    write_vector(
        "password-auth.json",
        "Password auth derivation (API-015, API-016): password_auth = HKDF-SHA256(salt = empty, info = \"auth\", ikm = argon2id(password UTF-8 bytes, salt, m, t, p, len 32)), 32 bytes. `salt_hex` is the opaque salt bytes (the API's `password_salt`, API-015). `password_auth_base64` is what goes over the wire (standard Base64); the hex form is the same bytes. `slow` marks cases with production-sized memory cost.",
        password_cases(),
    );
}
