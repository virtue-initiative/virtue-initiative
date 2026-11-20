# AGENTS.md — client/core

Shared Rust library used by all platform clients. **Read `architecture.md` before making non-trivial changes.** `SPEC.md` is the source-of-truth spec for the daemon loop itself.

## Design rule

Platform crates provide only raw screen data and OS hooks. `core` owns everything else: the daemon loop, persistence, retrying, hashing, batch construction, compression, encryption, and upload.

## Most dangerous files to edit

These files implement the client side of cross-component contracts shared with the TypeScript web app. Changing them incorrectly causes silent data corruption or decryption failures:

- `src/crypto.rs` — AES-256-GCM encryption, HPKE key wrapping, content hash computation
- `src/module/upload/batch.rs` — msgpack + gzip batch payload construction

See `../../AGENTS.md` (repo root) for the exact wire formats and constraints.

## Rules that are easy to break

- **Only the loop thread mutates `DaemonState`.** Public `Daemon` methods (`login`, `logout`, `flush_batch_now`, ...) send a `DaemonRequest` and block on the reply. Code running on the loop thread must call the private `apply_*` function instead, or it deadlocks waiting on its own reply. `status()` is the one direct read.
- **No locking inside a tick.** `run_phases` works on an owned clone of the state. Network I/O happens in the `execute_*` steps, which can't touch state; they hand results back for the `commit_*` steps.
- **Keep `PlatformHooks` minimal.** A new hook needs a reason no platform-independent code can provide. `PlatformHooks` is a blanket impl over `ScreenshotHooks + LifecycleHooks`; platforms implement those two.
- **The daemon must not depend on `ipc`.** `src/ipc.rs` exists only on Linux and macOS and gates itself with an inner `#![cfg]`, so the daemon stays free of per-OS code.
- **`event_state.json` must keep loading.** Its top-level keys (`auth`, `lifecycle`, `screenshot`, `upload`, `capture_availability`, `heartbeat`, `dns_filter`, `errors`) are what existing installs have on disk, and every module state is `#[serde(default)]`. A breaking change bumps `DaemonState.version` and adds its step to `DaemonState::upgrade`, which every load that a write may follow runs first (CORE-016, CORE-017).
- **Config is compile-time.** `src/config.rs` reads values `build.rs` bakes in with `env!()`. Don't add a runtime config file (CLIENT-001).

## Testing

Behavior tests drive a real `Daemon` through the `Scenario` DSL in `src/testing/`, with a mock API, a controllable clock and canned screenshots. Integration scenarios are in `tests/scenarios.rs`; per-module tests sit in each module file. Every test cites a `CORE-` ID (see `TESTING.md`).

```sh
cargo test -p virtue-core                      # unit tests only
cargo test -p virtue-core --features testing   # unit + integration tests
```
