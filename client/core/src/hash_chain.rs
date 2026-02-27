use sha2::{Digest, Sha256};

/// Maintains the rolling SHA-256 hash chain.
///
/// Each link: `hash[i] = SHA-256(hash[i-1] || image_sha256_or_zeros || unix_minute.to_le_bytes())`
///
/// `unix_minute` = `floor(unix_timestamp_seconds / 60)` as `u64`.
#[derive(Clone, Debug)]
pub struct ChainHasher {
    prev_hash: [u8; 32],
    start_hash: [u8; 32],
    initialized: bool,
}

impl Default for ChainHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl ChainHasher {
    /// Create a new hasher with an all-zeros genesis hash.
    pub fn new() -> Self {
        Self {
            prev_hash: [0u8; 32],
            start_hash: [0u8; 32],
            initialized: false,
        }
    }

    /// Compute the next chain hash and advance internal state.
    ///
    /// * `image_sha256` – SHA-256 of the capture taken in this minute, or `None` if no capture.
    /// * `unix_minute`  – `floor(unix_epoch_seconds / 60)`.
    pub fn next(&mut self, image_sha256: Option<&[u8; 32]>, unix_minute: u64) -> [u8; 32] {
        let zeros = [0u8; 32];
        let img = image_sha256.unwrap_or(&zeros);

        let mut h = Sha256::new();
        h.update(self.prev_hash);
        h.update(img);
        h.update(unix_minute.to_le_bytes());
        let hash: [u8; 32] = h.finalize().into();

        if !self.initialized {
            self.start_hash = hash;
            self.initialized = true;
        }
        self.prev_hash = hash;
        hash
    }

    /// Returns the first hash produced by this hasher (for `start_chain_hash` in batch uploads).
    /// Returns all-zeros if `next` has not been called yet.
    pub fn start_hash(&self) -> [u8; 32] {
        self.start_hash
    }

    /// Returns the most recently produced hash (for `end_chain_hash` in batch uploads).
    pub fn latest_hash(&self) -> [u8; 32] {
        self.prev_hash
    }

    /// Reset the hasher for a new batch window, seeding it with the last hash of the
    /// previous window so the chain is continuous across batch boundaries.
    pub fn reset_for_new_batch(&mut self) {
        self.start_hash = [0u8; 32];
        self.initialized = false;
        // prev_hash intentionally kept so chain is unbroken across batches
    }
}

#[cfg(test)]
mod tests {
    use super::ChainHasher;

    #[test]
    fn deterministic_chain() {
        let mut h1 = ChainHasher::new();
        let mut h2 = ChainHasher::new();

        let img = [0xabu8; 32];
        let r1a = h1.next(Some(&img), 100);
        let r1b = h2.next(Some(&img), 100);
        assert_eq!(r1a, r1b);

        let r2a = h1.next(None, 101);
        let r2b = h2.next(None, 101);
        assert_eq!(r2a, r2b);
    }

    #[test]
    fn start_hash_captured_correctly() {
        let mut h = ChainHasher::new();
        let first = h.next(None, 0);
        let _second = h.next(None, 1);
        assert_eq!(h.start_hash(), first);
    }

    #[test]
    fn chain_is_sensitive_to_image() {
        let mut h1 = ChainHasher::new();
        let mut h2 = ChainHasher::new();
        let img1 = [0xaau8; 32];
        let img2 = [0xbbu8; 32];
        assert_ne!(h1.next(Some(&img1), 5), h2.next(Some(&img2), 5));
    }
}
