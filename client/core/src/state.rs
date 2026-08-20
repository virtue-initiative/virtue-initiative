use std::fs::File;
use std::fs::OpenOptions;
use std::path::Path;

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
    Ok(serde_json::from_slice(&bytes)?)
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
/// the fd releases the `flock`/`LockFileEx` hold). See SPEC.md §7: this is
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
        .write(true)
        .open(&lock_path)?;
    file.lock_exclusive()?;
    Ok(StateLock { _file: file })
}
