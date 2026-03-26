use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::CoreResult;
use crate::model::{AuthState, BatchBufferState, DeviceSettings, PendingRequest, ServiceStatus};

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

    pub fn save_pending_requests(&self, requests: &[PendingRequest]) -> CoreResult<()> {
        self.write_json("pending_requests.json", requests)
    }

    pub fn load_pending_requests(&self) -> CoreResult<Vec<PendingRequest>> {
        Ok(self
            .read_json::<Vec<PendingRequest>>("pending_requests.json")?
            .unwrap_or_default())
    }

    pub fn save_auth_state(&self, auth_state: &AuthState) -> CoreResult<()> {
        self.write_json("auth.json", auth_state)
    }

    pub fn load_auth_state(&self) -> CoreResult<AuthState> {
        Ok(self.read_json("auth.json")?.unwrap_or_default())
    }

    pub fn save_batch_buffer(&self, batch_buffer: &BatchBufferState) -> CoreResult<()> {
        self.write_json("batch_buffer.json", batch_buffer)
    }

    pub fn load_batch_buffer(&self) -> CoreResult<BatchBufferState> {
        Ok(self.read_json("batch_buffer.json")?.unwrap_or_default())
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
