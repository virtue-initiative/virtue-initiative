use std::fs;
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::error::CoreResult;
use crate::model::{AuditRecord, AuthState, DeviceSettings, ServiceStatus};

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

    pub fn append_audit_record(&self, record: &AuditRecord) -> CoreResult<()> {
        let path = self.root.join("audit.jsonl");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        serde_json::to_writer(&mut file, record)?;
        writeln!(file)?;
        file.flush()?;
        Ok(())
    }

    pub fn load_audit_records(&self) -> CoreResult<Vec<AuditRecord>> {
        let path = self.root.join("audit.jsonl");
        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);
        let mut records = Vec::new();
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str(trimmed) {
                Ok(record) => records.push(record),
                Err(err) if err.is_eof() => break,
                Err(err) => return Err(err.into()),
            }
        }
        Ok(records)
    }

    pub fn clear_audit_records(&self) -> CoreResult<()> {
        let path = self.root.join("audit.jsonl");
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn save_device_settings(&self, settings: Option<&DeviceSettings>) -> CoreResult<()> {
        self.write_json("device_settings.json", &settings)
    }

    pub fn load_device_settings(&self) -> CoreResult<Option<DeviceSettings>> {
        Ok(self
            .read_json::<Option<DeviceSettings>>("device_settings.json")?
            .flatten())
    }

    pub fn append_error_log(&self, line: &str) -> CoreResult<()> {
        let path = self.root.join("errors.log");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(file, "{line}")?;
        Ok(())
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

        let bytes = fs::read(path)?;
        Ok(Some(serde_json::from_slice(&bytes)?))
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
    fn load_audit_records_ignores_partial_last_line() {
        let state_dir = temp_state_dir();
        let store = FileStateStore::new(&state_dir).expect("create store");
        let path = state_dir.join("audit.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"hash_uploaded\",\"local_id\":\"ok\"}\n",
                "{\"type\":\"log\""
            ),
        )
        .expect("write audit log");

        let records = store.load_audit_records().expect("load audit records");

        assert_eq!(records.len(), 1);
        match &records[0] {
            AuditRecord::HashUploaded { local_id } => assert_eq!(local_id, "ok"),
            other => panic!("unexpected record: {other:?}"),
        }

        let _ = fs::remove_dir_all(state_dir);
    }
}
