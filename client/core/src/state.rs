use std::fs::File;
use std::path::Path;

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
