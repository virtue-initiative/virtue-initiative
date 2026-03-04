use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use argon2::{Algorithm, Argon2, Params, Version};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;

use crate::error::{CoreError, CoreResult};

const PBKDF2_ITERATIONS: u32 = 100_000;
const NONCE_LEN: usize = 12;

/// Hash the password with argon2id before sending to the server.
/// Uses the lowercased email as a deterministic salt — matches the web client exactly.
/// NOTE: use the original (unhashed) password for the wrapping key.
pub fn hash_password_for_auth(password: &str, email: &str) -> CoreResult<String> {
    let salt = email.to_lowercase();
    let params = Params::new(65536, 3, 1, Some(32))
        .map_err(|e| CoreError::Crypto(format!("argon2 params error: {e}")))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut output = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt.as_bytes(), &mut output)
        .map_err(|e| CoreError::Crypto(format!("argon2 hash error: {e}")))?;
    Ok(hex::encode(output))
}

/// Derive a 32-byte AES key from the user's E2EE password and their user ID (used as salt).
pub fn derive_key(password: &str, user_id: &str) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(
        password.as_bytes(),
        user_id.as_bytes(),
        PBKDF2_ITERATIONS,
        &mut key,
    );
    key
}

/// Encrypt plaintext with AES-256-GCM.
/// Output format: `[12-byte nonce][ciphertext + 16-byte tag]`.
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> CoreResult<Vec<u8>> {
    let aes_key = Key::<Aes256Gcm>::from_slice(key);
    let cipher = Aes256Gcm::new(aes_key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| CoreError::Crypto(e.to_string()))?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt data produced by [`encrypt`].
pub fn decrypt(key: &[u8; 32], data: &[u8]) -> CoreResult<Vec<u8>> {
    if data.len() < NONCE_LEN {
        return Err(CoreError::Crypto("ciphertext too short".to_string()));
    }
    let aes_key = Key::<Aes256Gcm>::from_slice(key);
    let cipher = Aes256Gcm::new(aes_key);
    let nonce = Nonce::from_slice(&data[..NONCE_LEN]);

    cipher
        .decrypt(nonce, &data[NONCE_LEN..])
        .map_err(|e| CoreError::Crypto(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{decrypt, derive_key, encrypt};

    #[test]
    fn round_trip() {
        let key = derive_key("hunter2", "user-123");
        let plaintext = b"hello world";
        let ciphertext = encrypt(&key, plaintext).unwrap();
        let recovered = decrypt(&key, &ciphertext).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let key1 = derive_key("password1", "user-1");
        let key2 = derive_key("password2", "user-1");
        let ct = encrypt(&key1, b"secret").unwrap();
        assert!(decrypt(&key2, &ct).is_err());
    }
}
