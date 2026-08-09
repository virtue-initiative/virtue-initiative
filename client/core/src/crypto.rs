use aes_gcm::aead::{Aead, KeyInit, OsRng as AesOsRng, rand_core::RngCore};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use hkdf::Hkdf;
use hpke::{
    Deserializable, OpModeS, Serializable,
    aead::AesGcm256,
    kdf::HkdfSha256,
    kem::{Kem as KemTrait, X25519HkdfSha256},
    setup_sender,
};
use rand_core::{OsRng as HpkeOsRng, TryRngCore};
use sha2::{Digest, Sha256};

use crate::error::{CoreError, CoreResult};
use crate::model::{BatchRecipient, HashParams, LogEntry};

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

pub fn encode_batch_event(event: &LogEntry) -> CoreResult<Vec<u8>> {
    Ok(rmp_serde::to_vec_named(event)?)
}

pub fn compute_event_hash(encoded_event: &[u8]) -> [u8; 32] {
    Sha256::digest(encoded_event).into()
}

/// Generates a new Ed25519 device-signing keypair's raw private key bytes.
///
/// Deliberately does NOT call `SigningKey::generate()` — its `pkcs8`/
/// `rand_core` glue pins `rand_core 0.6`, which conflicts with this crate's
/// `rand_core = "0.9.5"` (used for HPKE). Reusing the OsRng/`fill_bytes`
/// pattern already used for the AES batch key above needs no `rand_core`
/// feature on `ed25519-dalek` at all.
pub fn generate_signing_key() -> [u8; 32] {
    let mut key = [0_u8; 32];
    AesOsRng.fill_bytes(&mut key);
    key
}

pub fn signing_public_key_base64(signing_key_bytes: &[u8; 32]) -> String {
    let signing_key = SigningKey::from_bytes(signing_key_bytes);
    base64::engine::general_purpose::STANDARD.encode(signing_key.verifying_key().to_bytes())
}

/// Signs a `POST /hash` request to the real hash-server. Message layout
/// (must stay byte-for-byte identical to `hash-server/src/auth.rs`'s
/// `verify_signature`):
///
/// `timestamp_ms (i64 LE, 8 bytes) || device_id || 0x00 || method || 0x00 || path || 0x00 || body`
pub fn sign_request(
    signing_key_bytes: &[u8; 32],
    timestamp_ms: i64,
    device_id: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> [u8; 64] {
    let signing_key = SigningKey::from_bytes(signing_key_bytes);

    let mut msg = Vec::with_capacity(
        8 + device_id.len() + 1 + method.len() + 1 + path.len() + 1 + body.len(),
    );
    msg.extend_from_slice(&timestamp_ms.to_le_bytes());
    msg.extend_from_slice(device_id.as_bytes());
    msg.push(0);
    msg.extend_from_slice(method.as_bytes());
    msg.push(0);
    msg.extend_from_slice(path.as_bytes());
    msg.push(0);
    msg.extend_from_slice(body);

    signing_key.sign(&msg).to_bytes()
}

#[cfg(test)]
mod tests {
    use super::{CryptoEngine, compute_event_hash, derive_password_auth};
    use crate::model::{BatchRecipient, HashParams};
    use aes_gcm::Aes256Gcm;
    use aes_gcm::Nonce;
    use aes_gcm::aead::{Aead, KeyInit};
    use base64::Engine;
    use hpke::aead::AesGcm256;
    use hpke::kdf::HkdfSha256;
    use hpke::kem::{Kem as KemTrait, X25519HkdfSha256};
    use hpke::{Deserializable, OpModeR, Serializable, setup_receiver};
    use rand_core::{OsRng as HpkeOsRng, TryRngCore};

    fn low_cost_params() -> HashParams {
        HashParams {
            version: "1".to_string(),
            algorithm: "argon2id".to_string(),
            memory_cost_kib: 64,
            time_cost: 1,
            parallelism: 1,
            salt_length: 16,
            hkdf_hash: "sha256".to_string(),
        }
    }

    #[test]
    fn generate_batch_key_produces_32_distinct_bytes() {
        let engine = CryptoEngine;
        let key1 = engine.generate_batch_key();
        let key2 = engine.generate_batch_key();
        assert_ne!(key1, [0_u8; 32], "key must not be all-zero");
        assert_ne!(key1, key2, "two keys must differ");
    }

    #[test]
    fn encrypt_batch_blob_nonce_is_first_12_bytes() {
        let engine = CryptoEngine;
        let key = engine.generate_batch_key();
        let plaintext = b"hello, world!";
        let blob = engine.encrypt_batch_blob(&key, plaintext).expect("encrypt");
        assert_eq!(blob.len(), 12 + plaintext.len() + 16);
    }

    #[test]
    fn encrypt_batch_blob_round_trips() {
        let engine = CryptoEngine;
        let key = engine.generate_batch_key();
        let plaintext = b"test plaintext for round-trip";
        let blob = engine.encrypt_batch_blob(&key, plaintext).expect("encrypt");
        let nonce = Nonce::from_slice(&blob[..12]);
        let cipher = Aes256Gcm::new_from_slice(&key).expect("valid key");
        let decrypted = cipher.decrypt(nonce, &blob[12..]).expect("decrypt");
        assert_eq!(decrypted.as_slice(), plaintext);
    }

    #[test]
    fn derive_password_auth_is_deterministic() {
        let params = low_cost_params();
        let out1 =
            derive_password_auth("testpassword", b"user@example.com", &params).expect("first call");
        let out2 = derive_password_auth("testpassword", b"user@example.com", &params)
            .expect("second call");
        assert_eq!(out1, out2);
        assert_ne!(out1, [0_u8; 32]);
    }

    #[test]
    fn derive_password_auth_changes_with_different_inputs() {
        let params = low_cost_params();
        let out1 = derive_password_auth("password1", b"user@example.com", &params).expect("call 1");
        let out2 = derive_password_auth("password2", b"user@example.com", &params).expect("call 2");
        assert_ne!(out1, out2);
    }

    #[test]
    fn compute_event_hash_is_deterministic() {
        let h1 = compute_event_hash(b"hello");
        let h2 = compute_event_hash(b"hello");
        assert_eq!(h1, h2);
        let h3 = compute_event_hash(b"world");
        assert_ne!(h1, h3);
    }

    #[test]
    fn wrap_and_unwrap_batch_key_round_trips() {
        let mut csprng = HpkeOsRng.unwrap_err();
        let (private_key, public_key) = <X25519HkdfSha256 as KemTrait>::gen_keypair(&mut csprng);
        let pub_key_bytes = public_key.to_bytes();
        let pub_key_b64 = base64::engine::general_purpose::STANDARD.encode(&pub_key_bytes[..]);

        let engine = CryptoEngine;
        let batch_key = engine.generate_batch_key();
        let recipient = BatchRecipient {
            user_id: "test-user".to_string(),
            pub_key_base64: pub_key_b64,
        };

        let wrapped = engine
            .wrap_batch_key_for_recipient(&recipient, &batch_key)
            .expect("wrap must succeed");

        let envelope = base64::engine::general_purpose::STANDARD
            .decode(&wrapped)
            .expect("base64 decode");

        // X25519 encapped key is 32 bytes
        let encapped_key =
            <<X25519HkdfSha256 as KemTrait>::EncappedKey as Deserializable>::from_bytes(
                &envelope[..32],
            )
            .expect("deserialize encapped key");

        let mut receiver_ctx = setup_receiver::<AesGcm256, HkdfSha256, X25519HkdfSha256>(
            &OpModeR::Base,
            &private_key,
            &encapped_key,
            b"",
        )
        .expect("setup_receiver must succeed");

        let recovered = receiver_ctx.open(&envelope[32..], b"").expect("open");
        assert_eq!(recovered, batch_key.to_vec());
    }
}
