//! Property tests for the batch format (`batch-format/SPEC.md`).
//!
//! Each property runs over generated inputs where the golden vectors in
//! `tests/vectors.rs` only cover chosen ones. Case counts are kept low so the
//! file runs in a few seconds. A failing case is saved by proptest under
//! `proptest-regressions/` and replayed on the next run.

use std::collections::BTreeMap;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use hpke::aead::AesGcm256;
use hpke::kdf::HkdfSha256;
use hpke::kem::{Kem as KemTrait, X25519HkdfSha256};
use hpke::{Deserializable, OpModeR, Serializable, setup_receiver};
use proptest::prelude::*;
use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::Value;
use sha2::{Digest, Sha256};
use virtue_core::crypto::{CryptoEngine, compute_event_hash, encode_batch_event};
use virtue_core::model::{BatchRecipient, LogEntry, ScreenshotSkipReason, UploadKind};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

/// The two required keys of an event, with the types BATCH-003 gives them.
#[derive(Deserialize)]
struct Typed {
    ts: i64,
    #[serde(rename = "type")]
    kind: String,
}

/// A score in `[0.0, 1.0]`, the range BATCH-003 allows for `risk`.
fn score() -> impl Strategy<Value = f32> {
    0.0f32..=1.0f32
}

fn upload_kind() -> impl Strategy<Value = UploadKind> {
    prop_oneof![
        (
            prop::collection::vec(any::<u8>(), 0..300),
            "[a-z]{1,8}/[a-z0-9.+-]{1,12}",
            prop::option::of(score()),
            prop::option::of(score()),
        )
            .prop_map(|(image, content_type, skin_detection, nsfw_detection)| {
                UploadKind::Screenshot {
                    image,
                    content_type,
                    skin_detection,
                    nsfw_detection,
                    model_runs: None,
                    images: None,
                }
            }),
        Just(UploadKind::UserStop),
        Just(UploadKind::UserStart),
        prop_oneof![
            Just(ScreenshotSkipReason::StaticScreen),
            Just(ScreenshotSkipReason::LockedOrScreensaver),
        ]
        .prop_map(|reason| UploadKind::ScreenshotSkipped { reason }),
        any::<String>().prop_map(|message| UploadKind::Alert { message }),
        Just(UploadKind::CaptureFailed),
        (any::<String>(), prop::option::of(any::<String>()))
            .prop_map(|(title, details)| UploadKind::Dev { title, details }),
        Just(UploadKind::Heartbeat),
        Just(UploadKind::ScreenshotMissed),
        any::<i64>().prop_map(|utc_ms| UploadKind::SystemLogin { utc_ms }),
        any::<i64>().prop_map(|utc_ms| UploadKind::SystemLogout { utc_ms }),
        (any::<u32>(), any::<i64>())
            .prop_map(|(count, window_ms)| UploadKind::RepeatedRestarts { count, window_ms }),
    ]
}

fn log_entry() -> impl Strategy<Value = LogEntry> {
    (any::<i64>(), prop::option::of(score()), upload_kind())
        .prop_map(|(ts, risk, event)| LogEntry { ts, risk, event })
}

fn as_json(entry: &LogEntry) -> Value {
    serde_json::to_value(entry).expect("entry serializes")
}

// BATCH-003: an event is a msgpack map with string keys `ts`, `type`, and
// optionally `risk` and `data`. Decoding the bytes gives the event back.
proptest! {
    #![proptest_config(config())]

    // BATCH-003: an event decodes to itself and has only the spec's keys.
    #[test]
    fn encoded_events_decode_to_the_same_event(entry in log_entry()) {
        let bytes = encode_batch_event(&entry).expect("encode");

        let decoded: LogEntry = rmp_serde::from_slice(&bytes).expect("decode");
        prop_assert_eq!(as_json(&decoded), as_json(&entry));

        // Re-encoding the decoded event gives the same bytes, so a consumer
        // that hashes what it received sees what the producer hashed.
        prop_assert_eq!(encode_batch_event(&decoded).expect("re-encode"), bytes.clone());

        // The map has only the keys the spec allows, with the spec's types.
        let map: BTreeMap<String, IgnoredAny> =
            rmp_serde::from_slice(&bytes).expect("an event is a map with string keys");
        prop_assert!(map.keys().all(|k| ["ts", "risk", "type", "data"].contains(&k.as_str())));
        prop_assert!(map.contains_key("ts") && map.contains_key("type"));
        prop_assert_eq!(map.contains_key("risk"), entry.risk.is_some());
        let typed: Typed = rmp_serde::from_slice(&bytes).expect("ts is an integer, type a string");
        prop_assert_eq!(typed.ts, entry.ts);
        prop_assert!(!typed.kind.is_empty());
    }
}

// BATCH-006: the content hash is the SHA-256 of the encoded event bytes, as
// they are, whatever those bytes are.
proptest! {
    #![proptest_config(config())]

    // BATCH-006: the content hash is sha256 of the bytes.
    #[test]
    fn content_hash_is_sha256_of_the_bytes(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let expected: [u8; 32] = Sha256::digest(&bytes).into();
        prop_assert_eq!(compute_event_hash(&bytes), expected);
    }

    // BATCH-006: changing any byte of an event changes its hash.
    #[test]
    fn content_hash_of_an_encoded_event_covers_every_byte(
        entry in log_entry(),
        position in any::<prop::sample::Index>(),
        flip in 1u8..=255,
    ) {
        let bytes = encode_batch_event(&entry).expect("encode");
        let mut changed = bytes.clone();
        let i = position.index(changed.len());
        changed[i] ^= flip;
        prop_assert_ne!(compute_event_hash(&bytes), compute_event_hash(&changed));
    }
}

// BATCH-002: the blob is `nonce[12] || ciphertext || tag[16]` under
// AES-256-GCM with no associated data. Any change to it MUST fail to decrypt.
proptest! {
    #![proptest_config(config())]

    // BATCH-002: the blob is nonce || ciphertext || tag, and changes fail.
    #[test]
    fn blob_decrypts_to_the_plaintext_and_rejects_changes(
        key in any::<[u8; 32]>(),
        plaintext in prop::collection::vec(any::<u8>(), 0..1024),
        position in any::<prop::sample::Index>(),
        flip in 1u8..=255,
    ) {
        let engine = CryptoEngine;
        let blob = engine.encrypt_batch_blob(&key, &plaintext).expect("encrypt");
        prop_assert_eq!(blob.len(), 12 + plaintext.len() + 16);

        let cipher = Aes256Gcm::new_from_slice(&key).expect("32-byte key");
        let opened = cipher.decrypt(Nonce::from_slice(&blob[..12]), &blob[12..]).expect("decrypt");
        prop_assert_eq!(opened, plaintext);

        let mut changed = blob.clone();
        let i = position.index(changed.len());
        changed[i] ^= flip;
        prop_assert!(cipher.decrypt(Nonce::from_slice(&changed[..12]), &changed[12..]).is_err());
    }

    // BATCH-002: each blob gets a fresh nonce.
    #[test]
    fn blobs_of_the_same_plaintext_use_a_fresh_nonce(
        key in any::<[u8; 32]>(),
        plaintext in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let engine = CryptoEngine;
        let a = engine.encrypt_batch_blob(&key, &plaintext).expect("encrypt");
        let b = engine.encrypt_batch_blob(&key, &plaintext).expect("encrypt");
        prop_assert_ne!(&a[..12], &b[..12]);
    }
}

// BATCH-005: the wrapped key is `enc[32] || ciphertext` (80 bytes) in padded
// Base64, and the recipient's private key opens it to the batch key.
proptest! {
    #![proptest_config(ProptestConfig { cases: 24, failure_persistence: None, ..ProptestConfig::default() })]

    // BATCH-005: the recipient opens the wrapped key.
    #[test]
    fn wrapped_batch_key_opens_with_the_recipient_private_key(
        private_key_bytes in any::<[u8; 32]>(),
        batch_key in any::<[u8; 32]>(),
    ) {
        let sk = <<X25519HkdfSha256 as KemTrait>::PrivateKey as Deserializable>::from_bytes(
            &private_key_bytes,
        )
        .expect("any 32 bytes are an X25519 private key");
        let recipient = BatchRecipient {
            user_id: "recipient".to_string(),
            pub_key_base64: B64.encode(X25519HkdfSha256::sk_to_pk(&sk).to_bytes()),
        };

        let wrapped = CryptoEngine
            .wrap_batch_key_for_recipient(&recipient, &batch_key)
            .expect("wrap");
        let envelope = B64.decode(&wrapped).expect("standard padded base64");
        prop_assert_eq!(envelope.len(), 32 + 32 + 16);

        let enc = <<X25519HkdfSha256 as KemTrait>::EncappedKey as Deserializable>::from_bytes(
            &envelope[..32],
        )
        .expect("encapsulated key");
        let mut ctx = setup_receiver::<AesGcm256, HkdfSha256, X25519HkdfSha256>(
            &OpModeR::Base,
            &sk,
            &enc,
            b"",
        )
        .expect("receiver setup");
        prop_assert_eq!(ctx.open(&envelope[32..], b"").expect("open"), batch_key.to_vec());
    }
}
