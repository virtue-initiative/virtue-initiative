use aes_gcm::aead::{Aead, KeyInit, OsRng as AesOsRng, rand_core::RngCore};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use hkdf::Hkdf;
use hpke::{
    Deserializable, Serializable,
    aead::AesGcm256,
    kdf::HkdfSha256,
    kem::{Kem as KemTrait, X25519HkdfSha256},
    setup_sender,
    OpModeS,
};
use rand_core::{OsRng as HpkeOsRng, TryRngCore};
use sha2::{Digest, Sha256};

use crate::error::{CoreError, CoreResult};
use crate::model::{BatchEvent, BatchEventData, BatchRecipient, BufferedScreenshot, HashParams, Screenshot};

type HpkeKem = X25519HkdfSha256;
type HpkeKdf = HkdfSha256;
type HpkeAead = AesGcm256;

#[derive(Debug, Clone, Default)]
pub struct CryptoEngine;

impl CryptoEngine {
    pub fn encrypt_batch_blob(&self, batch_key: &[u8; 32], plaintext: &[u8]) -> CoreResult<Vec<u8>> {
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
        let public_key_bytes = base64::engine::general_purpose::STANDARD
            .decode(&recipient.pub_key_base64)?;
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

pub fn prepare_screenshot_event(screenshot: Screenshot) -> BufferedScreenshot {
    let event = BatchEvent {
        ts: screenshot.captured_at_ms,
        kind: "screenshot".to_string(),
        data: BatchEventData {
            image: screenshot.bytes,
            content_type: screenshot.content_type,
        },
    };

    BufferedScreenshot {
        content_hash: compute_event_hash(&event),
        event,
    }
}

pub fn compute_event_hash(event: &BatchEvent) -> [u8; 32] {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(event.ts as u64).to_le_bytes());
    bytes.extend_from_slice(event.kind.as_bytes());

    let mut fields = vec![
        ("content_type", BatchValue::String(&event.data.content_type)),
        ("image", BatchValue::Bytes(&event.data.image)),
    ];
    fields.sort_by(|(left, _), (right, _)| left.cmp(right));

    for (key, value) in fields {
        bytes.extend_from_slice(key.as_bytes());
        match value {
            BatchValue::String(value) => bytes.extend_from_slice(value.as_bytes()),
            BatchValue::Bytes(value) => bytes.extend_from_slice(value),
        }
    }

    Sha256::digest(bytes).into()
}

enum BatchValue<'a> {
    String(&'a str),
    Bytes(&'a [u8]),
}
