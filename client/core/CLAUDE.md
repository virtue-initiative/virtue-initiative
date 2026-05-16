# CLAUDE.md — client/core

Shared Rust library used by all platform clients. **Read `architecture.md` before making non-trivial changes.**

## Design rule

Platform crates provide only raw screen data. `core` owns everything else: request flow, persistence, retrying, hashing, batch construction, compression, encryption, and upload.

## Most dangerous files to edit

These three files implement the client side of cross-component contracts shared with the TypeScript web app. Changing them incorrectly causes silent data corruption or decryption failures:

- `src/crypto.rs` — AES-256-GCM encryption, HPKE key wrapping, content hash computation
- `src/batch.rs` — msgpack + gzip batch payload construction
- `src/service.rs` — main loop, event kinds, lifecycle transitions

See `../CLAUDE.md` (repo root) for the exact wire formats and constraints.

## PlatformHooks

Keep the trait minimal. Platforms implement only:

```rust
take_screenshot() -> Result<Screenshot>
get_time_utc_ms() -> Result<i64>
```

Everything else belongs in `core`.

## State files (under `Config.state_dir`)

- `audit.jsonl` — append-only retry source; do not truncate or rewrite
- `auth.json` — device credentials
- `device_settings.json` — cached recipient public keys
- `status.json` — runtime status
- `errors.log` — permanent failures (400 responses)
