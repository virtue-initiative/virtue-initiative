use std::fs::File;
use std::fs::OpenOptions;
use std::path::Path;
use std::path::PathBuf;

use fs2::FileExt;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::CoreResult;

pub fn load_state<T: Default + DeserializeOwned>(path: &Path) -> CoreResult<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    let bytes = std::fs::read(path)?;
    if bytes.is_empty() {
        return Ok(T::default());
    }
    // CORE-017: a file that exists but won't parse (crash-truncated, corrupted,
    // or left behind by an incompatible build) is treated like a missing one
    // rather than a fatal error — see CORE-017 for why.
    match serde_json::from_slice(&bytes) {
        Ok(state) => Ok(state),
        Err(err) => {
            tracing::error!(
                path = %path.display(),
                error = %err,
                "state file exists but failed to parse; falling back to default"
            );
            // CORE-017: preserve the broken file's bytes before they're lost to the
            // next write of default-initialized state, so the failure is diagnosable.
            match backup_unparseable_state(path, &bytes) {
                Ok(backup_path) => tracing::error!(
                    backup_path = %backup_path.display(),
                    "backed up unparseable state file for debugging"
                ),
                Err(backup_err) => tracing::error!(
                    path = %path.display(),
                    error = %backup_err,
                    "failed to back up unparseable state file"
                ),
            }
            Ok(T::default())
        }
    }
}

/// Copies an unparseable state file's original bytes to a sibling path
/// (`<name>.corrupt-<unix_ms>`) so they survive the next persisted write of
/// default-initialized state. Best-effort — see CORE-017.
fn backup_unparseable_state(path: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut backup_name = path.file_name().unwrap_or_default().to_os_string();
    backup_name.push(format!(".corrupt-{timestamp}"));
    let backup_path = path.with_file_name(backup_name);
    std::fs::write(&backup_path, bytes)?;
    Ok(backup_path)
}

/// Write `state` atomically via a tmp-file + rename.
pub fn store_state<T: Serialize>(path: &Path, state: &T) -> CoreResult<()> {
    let tmp = path.with_extension("tmp");
    let file = File::create(&tmp)?;
    if let Err(e) = serde_json::to_writer(file, state) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Holds an OS-level advisory exclusive lock on `path`'s sibling `.lock`
/// file for as long as it's alive — released automatically on drop (closing
/// the fd releases the `flock`/`LockFileEx` hold). See CORE-016: this is
/// what serializes two processes (e.g. iOS's Safari-extension daemon and the
/// app's on-demand daemon) from racing a read-modify-write of the same
/// `state_path`. Blocks until the lock is available.
pub struct StateLock {
    _file: File,
}

pub fn lock_state(path: &Path) -> CoreResult<StateLock> {
    let lock_path = path.with_extension("lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&lock_path)?;
    file.lock_exclusive()?;
    Ok(StateLock { _file: file })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Default, Serialize, Deserialize, PartialEq, Debug)]
    struct Sample {
        value: u32,
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "virtue-state-test-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    /// Finds the single `<name>.corrupt-*` backup file `load_state` should have
    /// written next to `path`, and removes both it and `path`.
    fn take_backup_and_cleanup(path: &Path) -> Vec<u8> {
        let dir = path.parent().unwrap();
        let prefix = format!("{}.corrupt-", path.file_name().unwrap().to_str().unwrap());
        let backups: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(&prefix))
            })
            .collect();
        assert_eq!(
            backups.len(),
            1,
            "expected exactly one backup file for {}",
            path.display()
        );
        let backup_path = backups[0].path();
        let contents = std::fs::read(&backup_path).unwrap();
        let _ = std::fs::remove_file(&backup_path);
        let _ = std::fs::remove_file(path);
        contents
    }

    // CORE-017
    #[test]
    fn load_state_falls_back_to_default_when_file_is_unparseable() {
        let path = temp_path("corrupt.json");
        let original_bytes = b"{not valid json at all";
        std::fs::write(&path, original_bytes).unwrap();

        let loaded: Sample = load_state(&path).expect("must not error on unparseable state");

        assert_eq!(loaded, Sample::default());
        assert_eq!(take_backup_and_cleanup(&path), original_bytes);
    }

    // CORE-017
    #[test]
    fn load_state_falls_back_to_default_when_schema_is_incompatible() {
        let path = temp_path("incompatible.json");
        // Valid JSON, but not the shape `Sample` expects (e.g. an older/newer build's schema).
        let original_bytes = br#"{"value":"not-a-number","extra":true}"#;
        std::fs::write(&path, original_bytes).unwrap();

        let loaded: Sample = load_state(&path).expect("must not error on incompatible schema");

        assert_eq!(loaded, Sample::default());
        assert_eq!(take_backup_and_cleanup(&path), original_bytes);
    }

    #[test]
    fn load_state_round_trips_valid_state() {
        let path = temp_path("valid.json");
        let original = Sample { value: 42 };
        store_state(&path, &original).unwrap();

        let loaded: Sample = load_state(&path).unwrap();

        assert_eq!(loaded, original);
        let _ = std::fs::remove_file(&path);
    }

    // CORE-017
    #[test]
    fn load_state_does_not_back_up_a_valid_file() {
        let path = temp_path("valid-no-backup.json");
        store_state(&path, &Sample { value: 7 }).unwrap();

        let _: Sample = load_state(&path).unwrap();

        let dir = path.parent().unwrap();
        let prefix = format!("{}.corrupt-", path.file_name().unwrap().to_str().unwrap());
        let has_backup = std::fs::read_dir(dir).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(&prefix))
        });
        assert!(!has_backup, "a valid state file must not produce a backup");
        let _ = std::fs::remove_file(&path);
    }
}
