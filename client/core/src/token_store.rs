use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

pub trait TokenStore: Send + Sync {
    fn get_access_token(&self) -> CoreResult<Option<String>>;
    fn set_access_token(&self, token: &str) -> CoreResult<()>;
    fn clear_access_token(&self) -> CoreResult<()>;
}

#[derive(Debug, Default)]
pub struct MemoryTokenStore {
    token: Mutex<Option<String>>,
}

impl MemoryTokenStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TokenStore for MemoryTokenStore {
    fn get_access_token(&self) -> CoreResult<Option<String>> {
        let guard = self
            .token
            .lock()
            .map_err(|_| CoreError::TokenStore("memory token lock poisoned".to_string()))?;
        Ok(guard.clone())
    }

    fn set_access_token(&self, token: &str) -> CoreResult<()> {
        let mut guard = self
            .token
            .lock()
            .map_err(|_| CoreError::TokenStore("memory token lock poisoned".to_string()))?;
        *guard = Some(token.to_string());
        Ok(())
    }

    fn clear_access_token(&self) -> CoreResult<()> {
        let mut guard = self
            .token
            .lock()
            .map_err(|_| CoreError::TokenStore("memory token lock poisoned".to_string()))?;
        *guard = None;
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
        self.write_token_file(&StoredToken {
            access_token: Some(token.to_string()),
        })
    }

    fn clear_access_token(&self) -> CoreResult<()> {
        self.write_token_file(&StoredToken::default())
    }
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
        assert_eq!(
            store.get_access_token().expect("get token"),
            Some("abc".to_string())
        );

        let store2 = FileTokenStore::new(&path);
        assert_eq!(
            store2.get_access_token().expect("get token"),
            Some("abc".to_string())
        );

        store2.clear_access_token().expect("clear token");
        assert_eq!(store2.get_access_token().expect("get token"), None);
    }
}
