use std::fs::File;
use std::path::Path;

use crate::error::CoreResult;
use crate::events::StateType;

pub fn load_state(path: &Path) -> CoreResult<StateType> {
    if !path.exists() {
        return Ok(StateType::Null);
    }
    let bytes = std::fs::read(path)?;
    if bytes.is_empty() {
        return Ok(StateType::Null);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

/// Write `state` atomically via a tmp-file + rename.
pub fn store_state(path: &Path, state: &StateType) -> CoreResult<()> {
    let tmp = path.with_extension("tmp");
    let file = File::create(&tmp)?;
    if let Err(e) = serde_json::to_writer(file, state) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}
