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
    /// Hash all item fields deterministically:
    ///   id[16] || taken_at_le[8] || kind_utf8 || image_bytes || meta_k1 || meta_v1 || ...
    pub fn sha256(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        // id: UUID string → 16 raw bytes
        let id_hex: String = self.id.chars().filter(|c| *c != '-').collect();
        if id_hex.len() == 32 {
            let mut id_bytes = [0u8; 16];
            for i in 0..16 {
                if let Ok(b) = u8::from_str_radix(&id_hex[i * 2..i * 2 + 2], 16) {
                    id_bytes[i] = b;
                }
            }
            h.update(id_bytes);
        }
        // taken_at: 8-byte little-endian i64
        h.update(self.taken_at.to_le_bytes());
        // kind: raw UTF-8
        h.update(self.kind.as_bytes());
        // image: raw bytes (or nothing for missed captures)
        if let Some(img) = &self.image {
            h.update(img);
        }
        // metadata: each key then value as raw UTF-8, in order
        for (k, v) in &self.metadata {
            h.update(k.as_bytes());
            h.update(v.as_bytes());
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
    pub items: Vec<BatchItem>,
}

impl BatchBlob {
    pub fn new(items: Vec<BatchItem>) -> Self {
        Self { version: 1, items }
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
