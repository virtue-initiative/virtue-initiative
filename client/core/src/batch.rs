use serde::{Deserialize, Serialize};

/// A single captured item within a batch.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchItem {
    pub id: String,
    /// Unix timestamp in milliseconds.
    pub taken_at: i64,
    /// Event kind, e.g. "screenshot" or "missed_capture".
    pub kind: String,
    /// Raw image bytes (no base64). None for non-capture events.
    #[serde(with = "serde_bytes")]
    pub image: Option<Vec<u8>>,
    /// Key-value metadata pairs.
    pub metadata: Vec<(String, String)>,
}

impl BatchItem {
    pub fn sha256(&self) -> Option<[u8; 32]> {
        use sha2::{Digest, Sha256};
        self.image.as_ref().map(|bytes| {
            let mut h = Sha256::new();
            h.update(bytes);
            h.finalize().into()
        })
    }
}

/// The top-level structure serialised as MessagePack inside each R2 blob
/// (after gzip compression and AES-256-GCM encryption).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchBlob {
    /// Format version — currently 1.
    pub version: u8,
    pub items: Vec<BatchItem>,
}

impl BatchBlob {
    pub fn new(items: Vec<BatchItem>) -> Self {
        Self { version: 1, items }
    }

    /// Encode: msgpack → gzip → AES-256-GCM.
    pub fn encode_encrypted(&self, key: &[u8; 32]) -> crate::CoreResult<Vec<u8>> {
        use std::io::Write;
        use flate2::Compression;
        use flate2::write::GzEncoder;

        let msgpack = rmp_serde::to_vec_named(self).map_err(|e| {
            crate::CoreError::Serialization(e.to_string())
        })?;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&msgpack)?;
        let compressed = encoder.finish()?;

        crate::crypto::encrypt(key, &compressed)
    }

    /// Decode: AES-256-GCM → gzip → msgpack.
    pub fn decode_encrypted(data: &[u8], key: &[u8; 32]) -> crate::CoreResult<Self> {
        use std::io::Read;
        use flate2::read::GzDecoder;

        let compressed = crate::crypto::decrypt(key, data)?;

        let mut decoder = GzDecoder::new(&compressed[..]);
        let mut msgpack = Vec::new();
        decoder.read_to_end(&mut msgpack)?;

        rmp_serde::from_slice(&msgpack).map_err(|e| {
            crate::CoreError::Serialization(e.to_string())
        })
    }
}
