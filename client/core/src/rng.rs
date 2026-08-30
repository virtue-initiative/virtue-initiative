/// A source of uniform random draws, abstracted so screenshot cadence timing
/// can be driven deterministically in tests. See `module::screenshot::plan`.
pub trait RandomSource: Send + Sync + 'static {
    /// A uniform random value in `[0, 1)`.
    fn uniform(&self) -> f64;
}

/// Production `RandomSource`, backed by the OS CSPRNG.
pub struct OsRandomSource;

impl RandomSource for OsRandomSource {
    fn uniform(&self) -> f64 {
        use rand_core::{OsRng, TryRngCore};
        // 53 bits of entropy is the most an f64 mantissa can represent, so
        // draw a u64 and keep the top 53 bits. A failure of the OS RNG is
        // effectively unrecoverable elsewhere in the process too (crypto.rs
        // depends on it for key material) — 0.5 here just avoids a panic on
        // the very rare transient failure, at worst delaying/advancing the
        // next screenshot draw around its mean.
        let bits = OsRng.try_next_u64().unwrap_or(1u64 << 62);
        (bits >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}
