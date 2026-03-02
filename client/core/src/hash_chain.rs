use sha2::{Digest, Sha256};

/// Computes `sha256(a || b)` — mirrors what the server does when advancing device state.
/// Exposed for local testing/verification purposes only; the server is authoritative.
pub fn chain_step(current_state: &[u8; 32], content_hash: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(current_state);
    h.update(content_hash);
    h.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::chain_step;

    #[test]
    fn deterministic() {
        let state = [0u8; 32];
        let content = [0xabu8; 32];
        assert_eq!(chain_step(&state, &content), chain_step(&state, &content));
    }

    #[test]
    fn sensitive_to_content() {
        let state = [0u8; 32];
        assert_ne!(chain_step(&state, &[0xaau8; 32]), chain_step(&state, &[0xbbu8; 32]));
    }

    #[test]
    fn chained() {
        let c1 = [0x11u8; 32];
        let c2 = [0x22u8; 32];
        let s0 = [0u8; 32];
        let s1 = chain_step(&s0, &c1);
        let s2a = chain_step(&s1, &c2);
        // Different order → different result
        let s1b = chain_step(&s0, &c2);
        let s2b = chain_step(&s1b, &c1);
        assert_ne!(s2a, s2b);
    }
}
