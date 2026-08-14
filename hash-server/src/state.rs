use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub const ZERO_HASH: [u8; 32] = [0u8; 32];

#[derive(Debug, Clone, Copy)]
pub struct DeviceState {
    pub hash: [u8; 32],
    pub seq: u32,
    pub last_received: u32,
}

impl Default for DeviceState {
    fn default() -> Self {
        DeviceState {
            hash: ZERO_HASH,
            seq: 0,
            last_received: 0,
        }
    }
}

/// In-memory mirror of `device_hashes`, kept in lockstep with the database by
/// the writer thread (updated only after a transaction commits). GET reads
/// come from here so they never wait on the write queue or touch disk.
pub type SharedDevices = Arc<RwLock<HashMap<String, DeviceState>>>;

pub fn get(devices: &SharedDevices, device_id: &str) -> DeviceState {
    devices
        .read()
        .unwrap()
        .get(device_id)
        .copied()
        .unwrap_or_default()
}
