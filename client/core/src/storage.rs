use std::fs;
use std::path::{Path, PathBuf};

use crate::error::CoreResult;
use crate::model::{AuthState, ServiceStatus, StopIntent};

#[derive(Debug, Clone)]
pub struct FileStateStore {
    root: PathBuf,
}

impl FileStateStore {
    pub fn new(root: impl AsRef<Path>) -> CoreResult<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn save_status(&self, status: &ServiceStatus) -> CoreResult<()> {
        self.write_json("status.json", status)
    }

    pub fn load_status(&self) -> CoreResult<Option<ServiceStatus>> {
        self.read_json("status.json")
    }

    pub fn save_auth_state(&self, auth_state: &AuthState) -> CoreResult<()> {
        self.write_json("auth.json", auth_state)
    }

    pub fn load_auth_state(&self) -> CoreResult<AuthState> {
        Ok(self.read_json("auth.json")?.unwrap_or_default())
    }

    pub fn append_error_log(&self, line: &str) -> CoreResult<()> {
        use std::io::Write;
        let path = self.root.join("errors.log");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    pub fn save_stop_intent(&self, intent: &StopIntent) -> CoreResult<()> {
        self.write_json("stop_intent.json", &Some(intent))
    }

    pub fn load_stop_intent(&self) -> CoreResult<Option<StopIntent>> {
        Ok(self
            .read_json::<Option<StopIntent>>("stop_intent.json")?
            .flatten())
    }

    pub fn clear_stop_intent(&self) -> CoreResult<()> {
        let path = self.root.join("stop_intent.json");
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    fn write_json<T: serde::Serialize + ?Sized>(&self, name: &str, value: &T) -> CoreResult<()> {
        let path = self.root.join(name);
        let bytes = serde_json::to_vec_pretty(value)?;
        fs::write(path, bytes)?;
        Ok(())
    }

    fn read_json<T: serde::de::DeserializeOwned>(&self, name: &str) -> CoreResult<Option<T>> {
        let path = self.root.join(name);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path)?;
        Ok(Some(serde_json::from_slice(&bytes).map_err(|e| {
            crate::error::CoreError::SerdeContext {
                context: path.display().to_string(),
                source: e,
            }
        })?))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_state_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "virtue-storage-test-{}-{}",
            std::process::id(),
            TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temp state dir");
        path
    }

    #[test]
    fn stop_intent_round_trips_through_json_file() {
        let state_dir = temp_state_dir();
        let store = FileStateStore::new(&state_dir).expect("create store");
        let intent = StopIntent {
            source: "tray_close".to_string(),
            requested_at_ms: 1_234,
        };

        store.save_stop_intent(&intent).expect("save stop intent");
        let loaded = store
            .load_stop_intent()
            .expect("load stop intent")
            .expect("persisted stop intent");
        assert_eq!(loaded, intent);
        assert!(state_dir.join("stop_intent.json").exists());

        store.clear_stop_intent().expect("clear stop intent");
        assert!(store.load_stop_intent().expect("load cleared").is_none());
        assert!(!state_dir.join("stop_intent.json").exists());

        let _ = fs::remove_dir_all(state_dir);
    }
}
