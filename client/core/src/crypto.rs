use aes_gcm::aead::{Aead, KeyInit, OsRng, rand_core::RngCore};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use pbkdf2::pbkdf2_hmac_array;
use sha2::{Digest, Sha256};

use crate::error::{CoreError, CoreResult};
use crate::model::{BatchEvent, BatchEventData, BufferedScreenshot, Screenshot};

#[derive(Debug, Clone)]
pub struct CryptoEngine {
    key_bytes: [u8; 32],
}

impl CryptoEngine {
    pub fn from_base64(e2ee_key_base64: &str) -> CoreResult<Self> {
        let raw = base64::engine::general_purpose::STANDARD.decode(e2ee_key_base64)?;
        let key_bytes: [u8; 32] = raw
            .try_into()
            .map_err(|_| CoreError::InvalidState("e2ee_key must be 32 bytes"))?;
        Ok(Self { key_bytes })
    }

    pub fn encrypt_batch_blob(&self, plaintext: &[u8]) -> CoreResult<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(&self.key_bytes)
            .map_err(|_| CoreError::Crypto("invalid AES-256-GCM key"))?;
        let mut nonce_bytes = [0_u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| CoreError::Crypto("AES-256-GCM encryption failed"))?;

        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    pub fn decrypt_blob(&self, encrypted: &[u8]) -> CoreResult<Vec<u8>> {
        if encrypted.len() < 13 {
            return Err(CoreError::InvalidState(
                "AES-GCM payload must be nonce[12] || ciphertext+tag",
            ));
        }

        let cipher = Aes256Gcm::new_from_slice(&self.key_bytes)
            .map_err(|_| CoreError::Crypto("invalid AES-256-GCM key"))?;
        let nonce = Nonce::from_slice(&encrypted[..12]);
        cipher
            .decrypt(nonce, &encrypted[12..])
            .map_err(|_| CoreError::Crypto("AES-256-GCM decryption failed"))
    }
}

pub fn hash_password_for_auth(password: &str, email: &str) -> CoreResult<String> {
    let params = Params::new(65_536, 3, 1, Some(32))
        .map_err(|_| CoreError::InvalidState("invalid argon2 parameters"))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let salt = email.to_lowercase();
    let mut output = [0_u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt.as_bytes(), &mut output)
        .map_err(|err| CoreError::Argon2(err.to_string()))?;
    Ok(hex::encode(output))
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

pub fn derive_wrapping_key(password: &str, user_id: &str) -> [u8; 32] {
    pbkdf2_hmac_array::<Sha256, 32>(password.as_bytes(), user_id.as_bytes(), 100_000)
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
