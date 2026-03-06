use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BatchValue {
    String(String),
    Integer(i64),
    Boolean(bool),
    #[serde(with = "serde_bytes")]
    Binary(Vec<u8>),
}

/// A single captured item within a batch.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchItem {
    /// Unix timestamp in milliseconds.
    pub ts: i64,
    /// Event type, matching the API log shape.
    #[serde(rename = "type")]
    pub type_: String,
    /// Event payload, matching the API log `data` object shape.
    pub data: BTreeMap<String, BatchValue>,
}

impl BatchItem {
    /// Hash all item fields deterministically:
    ///   ts_le[8] || type_utf8 || data_key || data_value || ...
    pub fn sha256(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(self.ts.to_le_bytes());
        h.update(self.type_.as_bytes());
        for (key, value) in &self.data {
            h.update(key.as_bytes());
            match value {
                BatchValue::String(value) => h.update(value.as_bytes()),
                BatchValue::Integer(value) => h.update(value.to_le_bytes()),
                BatchValue::Boolean(value) => h.update([u8::from(*value)]),
                BatchValue::Binary(value) => h.update(value),
            }
        }
        h.finalize().into()
    }
}

/// The top-level structure serialised as MessagePack inside each R2 blob
/// (after gzip compression and AES-256-GCM encryption).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchBlob {
    /// Format version — currently 1.
    pub version: u8,
    pub events: Vec<BatchItem>,
}

impl BatchBlob {
    pub fn new(events: Vec<BatchItem>) -> Self {
        Self { version: 1, events }
    }

    /// Encode: msgpack → gzip → AES-256-GCM.
    pub fn encode_encrypted(&self, key: &[u8; 32]) -> crate::CoreResult<Vec<u8>> {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;

        let msgpack = rmp_serde::to_vec_named(self)
            .map_err(|e| crate::CoreError::Serialization(e.to_string()))?;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&msgpack)?;
        let compressed = encoder.finish()?;

        crate::crypto::encrypt(key, &compressed)
    }

    /// Decode: AES-256-GCM → gzip → msgpack.
    pub fn decode_encrypted(data: &[u8], key: &[u8; 32]) -> crate::CoreResult<Self> {
        use flate2::read::GzDecoder;
        use std::io::Read;

        let compressed = crate::crypto::decrypt(key, data)?;

        let mut decoder = GzDecoder::new(&compressed[..]);
        let mut msgpack = Vec::new();
        decoder.read_to_end(&mut msgpack)?;

        rmp_serde::from_slice(&msgpack).map_err(|e| crate::CoreError::Serialization(e.to_string()))
    }
}
