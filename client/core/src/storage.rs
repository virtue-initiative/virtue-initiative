use std::fs;
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};

use crate::error::CoreResult;
use crate::model::{
    AuditLogPayload, AuditRecord, AuthState, DeviceSettings, ServiceStatus, StoredAuditRecord,
};

const LEGACY_AUDIT_LOG_NAME: &str = "audit.jsonl";
const AUDIT_LOG_PREFIX: &str = "audit-";
const AUDIT_LOG_SUFFIX: &str = ".jsonl";
const COMPLETED_AUDIT_RETENTION_DAYS: i64 = 7;
const MAX_AUDIT_RETENTION_DAYS: i64 = 30;

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

    pub fn append_audit_log_record(&self, record: &AuditRecord) -> CoreResult<String> {
        let Some(audit_day) = audit_day_for_log_record(record) else {
            return Err(crate::error::CoreError::InvalidState(
                "audit log record missing timestamp",
            ));
        };
        self.append_audit_record_for_day(&audit_day, record)?;
        Ok(audit_day)
    }

    pub fn append_audit_record_for_day(
        &self,
        audit_day: &str,
        record: &AuditRecord,
    ) -> CoreResult<()> {
        self.append_record_to_path(&self.audit_log_path(audit_day), record)
    }

    pub fn load_audit_records(&self) -> CoreResult<Vec<StoredAuditRecord>> {
        self.load_audit_records_at(current_time_utc_ms()?)
    }

    pub fn load_audit_records_at(&self, now_ms: i64) -> CoreResult<Vec<StoredAuditRecord>> {
        self.migrate_legacy_audit_log(now_ms)?;
        self.prune_audit_logs(now_ms)?;

        let mut records = Vec::new();
        for path in self.audit_log_paths()? {
            let Some(audit_day) = audit_day_from_path(&path) else {
                continue;
            };
            for record in self.read_records_from_path(&path)? {
                records.push(StoredAuditRecord {
                    audit_day: audit_day.clone(),
                    record,
                });
            }
        }
        Ok(records)
    }

    pub fn clear_audit_records(&self) -> CoreResult<()> {
        for path in self.audit_log_paths()? {
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        let legacy_path = self.root.join(LEGACY_AUDIT_LOG_NAME);
        if legacy_path.exists() {
            fs::remove_file(legacy_path)?;
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

    fn append_record_to_path(&self, path: &Path, record: &AuditRecord) -> CoreResult<()> {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        serde_json::to_writer(&mut file, record)?;
        writeln!(file)?;
        file.flush()?;
        Ok(())
    }

    fn read_records_from_path(&self, path: &Path) -> CoreResult<Vec<AuditRecord>> {
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

    fn audit_log_path(&self, audit_day: &str) -> PathBuf {
        self.root
            .join(format!("{AUDIT_LOG_PREFIX}{audit_day}{AUDIT_LOG_SUFFIX}"))
    }

    fn audit_log_paths(&self) -> CoreResult<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if name.starts_with(AUDIT_LOG_PREFIX) && name.ends_with(AUDIT_LOG_SUFFIX) {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
    }

    fn migrate_legacy_audit_log(&self, now_ms: i64) -> CoreResult<()> {
        let legacy_path = self.root.join(LEGACY_AUDIT_LOG_NAME);
        if !legacy_path.exists() {
            return Ok(());
        }

        let records = self.read_records_from_path(&legacy_path)?;
        if records.is_empty() {
            fs::remove_file(legacy_path)?;
            return Ok(());
        }

        let fallback_day = file_modified_day(&legacy_path)
            .or_else(|| day_key_from_ms(now_ms).ok())
            .unwrap_or_else(|| "1970-01-01".to_string());

        let mut log_days = std::collections::HashMap::<String, String>::new();
        for record in &records {
            if let AuditRecord::Log { local_id, log, .. } = record {
                log_days.insert(local_id.clone(), day_key_from_payload(log)?);
            }
        }

        let mut grouped = std::collections::BTreeMap::<String, Vec<AuditRecord>>::new();
        for record in records {
            let audit_day = match &record {
                AuditRecord::Log { log, .. } => day_key_from_payload(log)?,
                AuditRecord::HashUploaded { local_id }
                | AuditRecord::LogUploaded { local_id, .. } => log_days
                    .get(local_id)
                    .cloned()
                    .unwrap_or_else(|| fallback_day.clone()),
                AuditRecord::BatchUploaded { .. } => fallback_day.clone(),
            };
            grouped.entry(audit_day).or_default().push(record);
        }

        for (audit_day, records) in grouped {
            let path = self.audit_log_path(&audit_day);
            for record in records {
                self.append_record_to_path(&path, &record)?;
            }
        }

        fs::remove_file(legacy_path)?;
        Ok(())
    }

    fn prune_audit_logs(&self, now_ms: i64) -> CoreResult<()> {
        let current_day = parse_day_key(&day_key_from_ms(now_ms)?)?;
        for path in self.audit_log_paths()? {
            let Some(audit_day) = audit_day_from_path(&path) else {
                continue;
            };
            let age_days = current_day
                .signed_duration_since(parse_day_key(&audit_day)?)
                .num_days();
            if age_days < 0 {
                continue;
            }
            if age_days >= MAX_AUDIT_RETENTION_DAYS {
                fs::remove_file(path)?;
                continue;
            }
            if age_days < COMPLETED_AUDIT_RETENTION_DAYS {
                continue;
            }
            let records = self.read_records_from_path(&path)?;
            if audit_day_fully_uploaded(&records) {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }
}

fn current_time_utc_ms() -> CoreResult<i64> {
    Ok(Utc::now().timestamp_millis())
}

fn audit_day_fully_uploaded(records: &[AuditRecord]) -> bool {
    let mut log_ids = std::collections::HashSet::<&str>::new();
    let mut uploaded_ids = std::collections::HashSet::<&str>::new();

    for record in records {
        match record {
            AuditRecord::Log { local_id, .. } => {
                log_ids.insert(local_id.as_str());
            }
            AuditRecord::LogUploaded { local_id, .. } => {
                uploaded_ids.insert(local_id.as_str());
            }
            AuditRecord::HashUploaded { .. } | AuditRecord::BatchUploaded { .. } => {}
        }
    }

    !log_ids.is_empty()
        && log_ids
            .iter()
            .all(|local_id| uploaded_ids.contains(local_id))
}

fn audit_day_for_log_record(record: &AuditRecord) -> Option<String> {
    match record {
        AuditRecord::Log { log, .. } => day_key_from_payload(log).ok(),
        _ => None,
    }
}

fn day_key_from_payload(payload: &AuditLogPayload) -> CoreResult<String> {
    let ts_ms = match payload {
        AuditLogPayload::Direct(log) => log.ts,
        AuditLogPayload::Batch(event) => event.event.ts,
    };
    day_key_from_ms(ts_ms)
}

fn day_key_from_ms(ts_ms: i64) -> CoreResult<String> {
    let date_time = DateTime::<Utc>::from_timestamp_millis(ts_ms).ok_or(
        crate::error::CoreError::InvalidState("invalid audit timestamp"),
    )?;
    Ok(date_time.format("%Y-%m-%d").to_string())
}

fn parse_day_key(value: &str) -> CoreResult<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| crate::error::CoreError::InvalidState("invalid audit day"))
}

fn audit_day_from_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let day = name
        .strip_prefix(AUDIT_LOG_PREFIX)?
        .strip_suffix(AUDIT_LOG_SUFFIX)?;
    Some(day.to_string())
}

fn file_modified_day(path: &Path) -> Option<String> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let modified = DateTime::<Utc>::from(modified);
    Some(modified.format("%Y-%m-%d").to_string())
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
        let expected_day = file_modified_day(&path).expect("legacy file modified day");

        let records = store.load_audit_records_at(0).expect("load audit records");

        assert_eq!(records.len(), 1);
        match &records[0].record {
            AuditRecord::HashUploaded { local_id } => assert_eq!(local_id, "ok"),
            other => panic!("unexpected record: {other:?}"),
        }
        assert_eq!(records[0].audit_day, expected_day);

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn load_audit_records_migrates_legacy_audit_log_by_day() {
        let state_dir = temp_state_dir();
        let store = FileStateStore::new(&state_dir).expect("create store");
        let path = state_dir.join("audit.jsonl");
        fs::write(
            &path,
            format!(
                concat!(
                    "{{\"type\":\"log\",\"local_id\":\"day-1\",\"should_be_in_batch\":false,\"log\":{{\"type\":\"direct\",\"data\":{{\"ts\":{},\"type\":\"system_event\",\"data\":{{}}}}}}}}\n",
                    "{{\"type\":\"log_uploaded\",\"local_id\":\"day-1\",\"server_id\":\"server-1\"}}\n",
                    "{{\"type\":\"log\",\"local_id\":\"day-2\",\"should_be_in_batch\":false,\"log\":{{\"type\":\"direct\",\"data\":{{\"ts\":{},\"type\":\"system_event\",\"data\":{{}}}}}}}}\n"
                ),
                86_400_000_i64,
                172_800_000_i64
            ),
        )
        .expect("write legacy audit log");

        let records = store
            .load_audit_records_at(172_800_000_i64)
            .expect("load migrated audit records");

        assert!(!path.exists());
        assert!(state_dir.join("audit-1970-01-02.jsonl").exists());
        assert!(state_dir.join("audit-1970-01-03.jsonl").exists());
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].audit_day, "1970-01-02");
        assert_eq!(records[1].audit_day, "1970-01-02");
        assert_eq!(records[2].audit_day, "1970-01-03");

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn load_audit_records_prunes_completed_days_after_local_retention() {
        let state_dir = temp_state_dir();
        let store = FileStateStore::new(&state_dir).expect("create store");
        let path = state_dir.join("audit-1970-01-01.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"log\",\"local_id\":\"done\",\"should_be_in_batch\":false,\"log\":{\"type\":\"direct\",\"data\":{\"ts\":0,\"type\":\"system_event\",\"data\":{}}}}\n",
                "{\"type\":\"log_uploaded\",\"local_id\":\"done\",\"server_id\":\"server-1\"}\n"
            ),
        )
        .expect("write rotated audit log");

        let records = store
            .load_audit_records_at(8 * 86_400_000_i64)
            .expect("load pruned audit records");

        assert!(records.is_empty());
        assert!(!path.exists());

        let _ = fs::remove_dir_all(state_dir);
    }

    #[test]
    fn load_audit_records_hard_prunes_old_unfinished_days() {
        let state_dir = temp_state_dir();
        let store = FileStateStore::new(&state_dir).expect("create store");
        let path = state_dir.join("audit-1970-01-01.jsonl");
        fs::write(
            &path,
            "{\"type\":\"log\",\"local_id\":\"stale\",\"should_be_in_batch\":false,\"log\":{\"type\":\"direct\",\"data\":{\"ts\":0,\"type\":\"system_event\",\"data\":{}}}}\n",
        )
        .expect("write rotated audit log");

        let records = store
            .load_audit_records_at(31 * 86_400_000_i64)
            .expect("load pruned audit records");

        assert!(records.is_empty());
        assert!(!path.exists());

        let _ = fs::remove_dir_all(state_dir);
    }
}
