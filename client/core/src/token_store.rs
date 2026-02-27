use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

pub trait TokenStore: Send + Sync {
    fn get_access_token(&self) -> CoreResult<Option<String>>;
    fn set_access_token(&self, token: &str) -> CoreResult<()>;
    fn clear_access_token(&self) -> CoreResult<()>;
    fn get_refresh_token(&self) -> CoreResult<Option<String>>;
    fn set_refresh_token(&self, token: &str) -> CoreResult<()>;
    fn clear_refresh_token(&self) -> CoreResult<()>;
    fn get_e2ee_key(&self) -> CoreResult<Option<[u8; 32]>>;
    fn set_e2ee_key(&self, key: &[u8; 32]) -> CoreResult<()>;
    fn clear_e2ee_key(&self) -> CoreResult<()>;
}

#[derive(Debug, Default)]
pub struct MemoryTokenStore {
    tokens: Mutex<StoredToken>,
}

impl MemoryTokenStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TokenStore for MemoryTokenStore {
    fn get_access_token(&self) -> CoreResult<Option<String>> {
        let guard = self
            .tokens
            .lock()
            .map_err(|_| CoreError::TokenStore("memory token lock poisoned".to_string()))?;
        Ok(guard.access_token.clone())
    }

    fn set_access_token(&self, token: &str) -> CoreResult<()> {
        let mut guard = self
            .tokens
            .lock()
            .map_err(|_| CoreError::TokenStore("memory token lock poisoned".to_string()))?;
        guard.access_token = Some(token.to_string());
        Ok(())
    }

    fn clear_access_token(&self) -> CoreResult<()> {
        let mut guard = self
            .tokens
            .lock()
            .map_err(|_| CoreError::TokenStore("memory token lock poisoned".to_string()))?;
        guard.access_token = None;
        Ok(())
    }

    fn get_refresh_token(&self) -> CoreResult<Option<String>> {
        let guard = self
            .tokens
            .lock()
            .map_err(|_| CoreError::TokenStore("memory token lock poisoned".to_string()))?;
        Ok(guard.refresh_token.clone())
    }

    fn set_refresh_token(&self, token: &str) -> CoreResult<()> {
        let mut guard = self
            .tokens
            .lock()
            .map_err(|_| CoreError::TokenStore("memory token lock poisoned".to_string()))?;
        guard.refresh_token = Some(token.to_string());
        Ok(())
    }

    fn clear_refresh_token(&self) -> CoreResult<()> {
        let mut guard = self
            .tokens
            .lock()
            .map_err(|_| CoreError::TokenStore("memory token lock poisoned".to_string()))?;
        guard.refresh_token = None;
        Ok(())
    }

    fn get_e2ee_key(&self) -> CoreResult<Option<[u8; 32]>> {
        let guard = self
            .tokens
            .lock()
            .map_err(|_| CoreError::TokenStore("memory token lock poisoned".to_string()))?;
        parse_e2ee_key_hex(guard.e2ee_key_hex.as_deref())
    }

    fn set_e2ee_key(&self, key: &[u8; 32]) -> CoreResult<()> {
        let mut guard = self
            .tokens
            .lock()
            .map_err(|_| CoreError::TokenStore("memory token lock poisoned".to_string()))?;
        guard.e2ee_key_hex = Some(hex::encode(key));
        Ok(())
    }

    fn clear_e2ee_key(&self) -> CoreResult<()> {
        let mut guard = self
            .tokens
            .lock()
            .map_err(|_| CoreError::TokenStore("memory token lock poisoned".to_string()))?;
        guard.e2ee_key_hex = None;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct FileTokenStore {
    path: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredToken {
    access_token: Option<String>,
    refresh_token: Option<String>,
    /// AES-256 key as lowercase hex (64 chars). Derived from the user's E2EE password.
    e2ee_key_hex: Option<String>,
}

impl FileTokenStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    fn read_token_file(&self) -> CoreResult<StoredToken> {
        if !self.path.exists() {
            return Ok(StoredToken::default());
        }

        let raw = fs::read(&self.path)?;
        if raw.is_empty() {
            return Ok(StoredToken::default());
        }

        Ok(serde_json::from_slice::<StoredToken>(&raw)?)
    }

    fn write_token_file(&self, token: &StoredToken) -> CoreResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let tmp_path = self.path.with_extension("tmp");
        let bytes = serde_json::to_vec(token)?;
        fs::write(&tmp_path, bytes)?;
        fs::rename(tmp_path, &self.path)?;
        Ok(())
    }
}

impl TokenStore for FileTokenStore {
    fn get_access_token(&self) -> CoreResult<Option<String>> {
        let token = self.read_token_file()?;
        Ok(token.access_token)
    }

    fn set_access_token(&self, token: &str) -> CoreResult<()> {
        let mut stored = self.read_token_file()?;
        stored.access_token = Some(token.to_string());
        self.write_token_file(&stored)
    }

    fn clear_access_token(&self) -> CoreResult<()> {
        let mut stored = self.read_token_file()?;
        stored.access_token = None;
        self.write_token_file(&stored)
    }

    fn get_refresh_token(&self) -> CoreResult<Option<String>> {
        let token = self.read_token_file()?;
        Ok(token.refresh_token)
    }

    fn set_refresh_token(&self, token: &str) -> CoreResult<()> {
        let mut stored = self.read_token_file()?;
        stored.refresh_token = Some(token.to_string());
        self.write_token_file(&stored)
    }

    fn clear_refresh_token(&self) -> CoreResult<()> {
        let mut stored = self.read_token_file()?;
        stored.refresh_token = None;
        self.write_token_file(&stored)
    }

    fn get_e2ee_key(&self) -> CoreResult<Option<[u8; 32]>> {
        let stored = self.read_token_file()?;
        parse_e2ee_key_hex(stored.e2ee_key_hex.as_deref())
    }

    fn set_e2ee_key(&self, key: &[u8; 32]) -> CoreResult<()> {
        let mut stored = self.read_token_file()?;
        stored.e2ee_key_hex = Some(hex::encode(key));
        self.write_token_file(&stored)
    }

    fn clear_e2ee_key(&self) -> CoreResult<()> {
        let mut stored = self.read_token_file()?;
        stored.e2ee_key_hex = None;
        self.write_token_file(&stored)
    }
}

fn parse_e2ee_key_hex(hex_str: Option<&str>) -> CoreResult<Option<[u8; 32]>> {
    let Some(s) = hex_str else { return Ok(None) };
    let bytes = hex::decode(s)
        .map_err(|e| CoreError::TokenStore(format!("invalid e2ee_key_hex: {e}")))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| CoreError::TokenStore("e2ee_key_hex must be 32 bytes".to_string()))?;
    Ok(Some(arr))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{FileTokenStore, TokenStore};

    #[test]
    fn file_store_persists_and_clears() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("token.json");

        let store = FileTokenStore::new(&path);
        store.set_access_token("abc").expect("set token");
        store
            .set_refresh_token("refresh")
            .expect("set refresh token");
        assert_eq!(
            store.get_access_token().expect("get token"),
            Some("abc".to_string())
        );
        assert_eq!(
            store.get_refresh_token().expect("get refresh token"),
            Some("refresh".to_string())
        );

        let store2 = FileTokenStore::new(&path);
        assert_eq!(
            store2.get_access_token().expect("get token"),
            Some("abc".to_string())
        );
        assert_eq!(
            store2.get_refresh_token().expect("get refresh token"),
            Some("refresh".to_string())
        );

        store2.clear_access_token().expect("clear token");
        assert_eq!(store2.get_access_token().expect("get token"), None);
        assert_eq!(
            store2.get_refresh_token().expect("get refresh token"),
            Some("refresh".to_string())
        );

        store2.clear_refresh_token().expect("clear refresh token");
        assert_eq!(store2.get_refresh_token().expect("get refresh token"), None);
    }
}
