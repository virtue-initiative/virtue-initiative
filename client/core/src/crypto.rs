use aes_gcm::aead::{Aead, KeyInit, OsRng as AesOsRng, rand_core::RngCore};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use hkdf::Hkdf;
use hpke::{
    Deserializable, OpModeS, Serializable,
    aead::AesGcm256,
    kdf::HkdfSha256,
    kem::{Kem as KemTrait, X25519HkdfSha256},
    setup_sender,
};
use rand_core::{OsRng as HpkeOsRng, TryRngCore};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::{CoreError, CoreResult};
use crate::model::{
    BatchEvent, BatchEventData, BatchRecipient, BufferedBatchEvent, HashParams, Screenshot,
};

type HpkeKem = X25519HkdfSha256;
type HpkeKdf = HkdfSha256;
type HpkeAead = AesGcm256;

#[derive(Debug, Clone, Default)]
pub struct CryptoEngine;

impl CryptoEngine {
    pub fn encrypt_batch_blob(
        &self,
        batch_key: &[u8; 32],
        plaintext: &[u8],
    ) -> CoreResult<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(batch_key)
            .map_err(|_| CoreError::Crypto("invalid AES-256-GCM key"))?;
        let mut nonce_bytes = [0_u8; 12];
        AesOsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| CoreError::Crypto("AES-256-GCM encryption failed"))?;

        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    pub fn generate_batch_key(&self) -> [u8; 32] {
        let mut batch_key = [0_u8; 32];
        AesOsRng.fill_bytes(&mut batch_key);
        batch_key
    }

    pub fn wrap_batch_key_for_recipient(
        &self,
        recipient: &BatchRecipient,
        batch_key: &[u8; 32],
    ) -> CoreResult<String> {
        let public_key_bytes =
            base64::engine::general_purpose::STANDARD.decode(&recipient.pub_key_base64)?;
        let public_key = <HpkeKem as KemTrait>::PublicKey::from_bytes(&public_key_bytes)
            .map_err(|_| CoreError::Crypto("invalid X25519 public key"))?;
        let mut csprng = HpkeOsRng.unwrap_err();
        let (encapped_key, mut sender) = setup_sender::<HpkeAead, HpkeKdf, HpkeKem, _>(
            &OpModeS::Base,
            &public_key,
            b"",
            &mut csprng,
        )
        .map_err(|_| CoreError::Crypto("HPKE setup failed"))?;
        let ciphertext = sender
            .seal(batch_key, b"")
            .map_err(|_| CoreError::Crypto("HPKE encryption failed"))?;
        let mut envelope = Vec::with_capacity(encapped_key.to_bytes().len() + ciphertext.len());
        envelope.extend_from_slice(encapped_key.to_bytes().as_slice());
        envelope.extend_from_slice(&ciphertext);
        Ok(base64::engine::general_purpose::STANDARD.encode(envelope))
    }
}

pub fn derive_password_auth(
    password: &str,
    password_salt: &[u8],
    params: &HashParams,
) -> CoreResult<[u8; 32]> {
    let argon_params = Params::new(
        params.memory_cost_kib,
        params.time_cost,
        params.parallelism,
        Some(32),
    )
    .map_err(|_| CoreError::InvalidState("invalid argon2 parameters"))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);
    let mut argon_output = [0_u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), password_salt, &mut argon_output)
        .map_err(|err| CoreError::Argon2(err.to_string()))?;

    let hkdf = Hkdf::<Sha256>::new(None, &argon_output);
    let mut password_auth = [0_u8; 32];
    hkdf.expand(b"auth", &mut password_auth)
        .map_err(|_| CoreError::Crypto("HKDF expand failed"))?;
    Ok(password_auth)
}

pub fn buffer_batch_event(event: BatchEvent) -> BufferedBatchEvent {
    BufferedBatchEvent {
        content_hash: compute_event_hash(&event),
        event,
    }
}

pub fn prepare_screenshot_event(screenshot: Screenshot) -> BufferedBatchEvent {
    prepare_screenshot_batch_event(screenshot, "screenshot", None, BatchEventData::default())
}

pub fn prepare_screenshot_batch_event(
    screenshot: Screenshot,
    kind: impl Into<String>,
    risk: Option<f32>,
    data: BatchEventData,
) -> BufferedBatchEvent {
    buffer_batch_event(BatchEvent {
        ts: screenshot.captured_at_ms,
        kind: kind.into(),
        risk,
        data: data.with_image(screenshot.bytes, screenshot.content_type),
    })
}

pub fn prepare_log_batch_event(
    ts: i64,
    kind: impl Into<String>,
    risk: Option<f32>,
    data: BatchEventData,
) -> BufferedBatchEvent {
    buffer_batch_event(BatchEvent {
        ts,
        kind: kind.into(),
        risk,
        data,
    })
}

pub fn compute_event_hash(event: &BatchEvent) -> [u8; 32] {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(event.ts as u64).to_le_bytes());
    bytes.extend_from_slice(event.kind.as_bytes());
    if let Some(risk) = event.risk {
        bytes.extend_from_slice(b"risk");
        bytes.extend_from_slice(&risk.to_bits().to_le_bytes());
    }

    if !event.data.content_type.is_empty() {
        bytes.extend_from_slice(b"content_type");
        bytes.extend_from_slice(event.data.content_type.as_bytes());
    }
    if !event.data.image.is_empty() {
        bytes.extend_from_slice(b"image");
        bytes.extend_from_slice(&event.data.image);
    }
    for (key, value) in &event.data.fields {
        bytes.extend_from_slice(key.as_bytes());
        append_json_value(&mut bytes, value);
    }

    Sha256::digest(bytes).into()
}

fn append_json_value(bytes: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Null => bytes.extend_from_slice(b"n"),
        Value::Bool(value) => {
            bytes.extend_from_slice(b"b");
            bytes.push(u8::from(*value));
        }
        Value::Number(value) => {
            bytes.extend_from_slice(b"#");
            bytes.extend_from_slice(value.to_string().as_bytes());
        }
        Value::String(value) => {
            bytes.extend_from_slice(b"s");
            bytes.extend_from_slice(value.as_bytes());
        }
        Value::Array(values) => {
            bytes.extend_from_slice(b"[");
            for value in values {
                append_json_value(bytes, value);
            }
            bytes.extend_from_slice(b"]");
        }
        Value::Object(values) => {
            bytes.extend_from_slice(b"{");
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort();
            for key in keys {
                bytes.extend_from_slice(key.as_bytes());
                append_json_value(bytes, &values[key]);
            }
            bytes.extend_from_slice(b"}");
        }
    }
}
