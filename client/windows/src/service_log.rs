use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Result;
use chrono::Utc;

#[derive(Debug)]
pub struct ServiceLogger {
    path: PathBuf,
    lock: Mutex<()>,
}

impl ServiceLogger {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    pub fn info(&self, message: &str) {
        let _ = self.write("INFO", message);
    }

    pub fn warn(&self, message: &str) {
        let _ = self.write("WARN", message);
    }

    pub fn error(&self, message: &str) {
        let _ = self.write("ERROR", message);
    }

    fn write(&self, level: &str, message: &str) -> Result<()> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| anyhow::anyhow!("log lock poisoned"))?;
        let timestamp = Utc::now().to_rfc3339();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "[{timestamp}] {level} {message}")?;
        Ok(())
    }
}
