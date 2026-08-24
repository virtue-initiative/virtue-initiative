# CLAUDE.md — client/core

Shared Rust library used by all platform clients. **Read `architecture.md` before making non-trivial changes.** `SPEC.md` is the source-of-truth spec for the daemon loop itself.

## Design rule

Platform crates provide only raw screen data and OS hooks. `core` owns everything else: the daemon loop, persistence, retrying, hashing, batch construction, compression, encryption, and upload.

## Most dangerous files to edit

These files implement the client side of cross-component contracts shared with the TypeScript web app. Changing them incorrectly causes silent data corruption or decryption failures:

- `src/crypto.rs` — AES-256-GCM encryption, HPKE key wrapping, content hash computation
- `src/module/upload/batch.rs` — msgpack + gzip batch payload construction

See `../CLAUDE.md` (repo root) for the exact wire formats and constraints.

## PlatformHooks

Keep the traits minimal. Platforms implement `ScreenshotHooks` (screen capture,
lock detection) and `LifecycleHooks` (the hooks the late-wakeup model needs):

```rust
// ScreenshotHooks
take_screenshot() -> Result<Screenshot>
get_time_utc_ms() -> Result<i64>
is_locked_or_screensaver() -> Result<bool>
can_force_capture_now() -> bool  // default: true

// LifecycleHooks
get_utc_clock_ms() -> Result<i64>
get_monotonic_clock_ms() -> Result<i64>  // default: falls back to get_utc_clock_ms
get_last_login_utc_ms() -> Result<Option<i64>>
get_last_logout_utc_ms() -> Result<Option<i64>>
```

`PlatformHooks: ScreenshotHooks + LifecycleHooks` is a blanket impl — platforms
never implement it directly. `get_monotonic_clock_ms` (a clock that doesn't
advance while suspended) feeds only `lifecycle::tick`'s suspend evidence
(CORE-002) — it's not on `ScreenshotHooks`, and screenshot scheduling
itself still paces off the wall clock. Mac separately keeps its own boot/
monotonic clock reads as **inherent** methods on `MacPlatformHooks` for a
local post-wake UX check unrelated to the core model — see `architecture.md`.

Everything else belongs in `core`.

## The daemon loop

`core` is a single sequential loop, not an event bus:

```rust
let daemon = Daemon::new(config, platform, api, state_path)?;
daemon.run_forever(); // blocking — call from its own thread
```

- `src/daemon.rs` — `Daemon<P, A>` / `DaemonState`. `run_forever` waits for
  either the next scheduled wakeup or an incoming `DaemonRequest`, applies
  and persists any requests that arrived, then clones the current state once
  and runs one tick (`run_phases`: lifecycle check, screenshot
  plan/capture/commit, capture-availability, heartbeat, hash retries, batch
  upload) against that owned clone with **no locking anywhere in the
  middle**, writing the result back to the shared snapshot and to disk at
  the end.
- `login`/`logout`/`note_user_stop`/`queue_upload`/`flush_batch_now`/
  `force_capture_now` are `Daemon` methods that build a `DaemonRequest`, send it on an `mpsc`
  channel, and block on a reply — the loop thread is the only place that
  ever mutates `DaemonState`. `status()` is the one exception: a direct
  lock-based read of the loop's last-committed snapshot, since there's
  nothing to synchronize. `request_stop()` stays fire-and-forget.
  `request_forced_capture()` is a third shape (iOS-only): it doesn't touch
  the request channel at all, just a direct `with_locked_state` call, so it
  works even when no loop is running in this process — see
  `../CLAUDE.md`'s iOS section.
- `src/ipc.rs` (Linux/macOS only) — the cross-process transport for the
  CLI/tray, sitting on top of the same `Daemon` methods; see "IPC" below.

## The 6 modules (`src/module/`)

Each is a plain `struct FooState` (serde default) plus free functions — no
trait, no event dispatch. A module that needs to enqueue work calls
`upload::enqueue(&mut upload_state, now_ms, risk, kind)` directly.

| Module                 | State                                                     | Key functions                                                                                                              |
| ---------------------- | --------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `auth`                 | (writes into `AuthState`/`ScreenshotState`/`UploadState`) | `login()`, `logout()`                                                                                                      |
| `lifecycle`            | `LifecycleState`                                          | `tick()` (late-wakeup check), `note_user_stop()`                                                                           |
| `screenshot`           | `ScreenshotState`                                         | `plan()`, `plan_forced()` (on-demand capture, bypasses the interval gate), `capture_and_process()`, `commit()`             |
| `upload`               | `UploadState`                                             | `enqueue()`, `plan_hash_retries`/`execute_hash_retries`/`commit_hash_retries`, `plan_batch`/`execute_batch`/`commit_batch` |
| `capture_availability` | `CaptureAvailabilityState`                                | `note_failure()`, `tick()`                                                                                                 |
| `heartbeat`            | `HeartbeatState`                                          | `tick()`                                                                                                                   |
| `status`               | —                                                         | `build()` (pure `ServiceStatus` assembly)                                                                                  |

`lifecycle::tick` compares actual vs. scheduled wakeup time each tick and
alerts on a single late wakeup > 1 min or a last-10-array sum > 5 min,
excused near a system login/logout — see CORE-002 and `tampering.md`
(now a short pointer to SPEC.md, not its own model).

## State persistence

`event_state.json` (in `Config.state_dir`) holds the serialized `DaemonState`.
Top-level field names match the pre-rewrite per-observer keys (`auth`,
`lifecycle`, `screenshot`, `upload`, `capture_availability`, `heartbeat`) —
existing installs load cleanly. `Daemon::new` loads it (or defaults) and, if
already authenticated, refreshes device settings once before returning.

## IPC (Linux/Mac only)

Windows/Android/iOS need no code here at all — each holds one process-global
`Arc<Daemon<..>>` and calls its methods directly, which is already the
"thread channel" described above. Linux and macOS additionally run their
CLI/tray as a separate OS process from the resident daemon, so they need a
real cross-process transport on top of those same methods:

- `src/ipc.rs` — single file. `spawn_server` binds a Unix socket and spawns
  one thread that loops `accept` -> serve that connection to completion
  (decode a newline-JSON `WireRequest`, call the matching `Daemon` method,
  encode the `WireReply`) -> `accept` again. Only one client is ever
  connected at a time; a second `connect()` just blocks until the first
  disconnects. `ClientController` is the client side: login, logout, status,
  and friends. The daemon only ever writes in reply to a request — there are
  no unprompted pushes, so the CLI/tray learns about a logout from its next
  `get_status()` poll.
- `ipc.rs` gates *itself* with an inner `#![cfg(any(target_os = "linux",
  target_os = "macos"))]`, so `target_os` appears in exactly one file in the
  crate. `lib.rs` declares `pub mod ipc;` unconditionally (it's simply empty
  elsewhere) and does not re-export its types — consumers name
  `virtue_core::ipc::{ClientController, spawn_server}`, which keeps the
  platform-conditional part of the API visibly scoped to the module that is
  itself conditional. Nothing else in `core` may depend on `ipc`; that
  coupling is what previously leaked `target_os` into `daemon.rs`.

## Testing

- `src/testing/` — `MockApiClient`, `TestPlatformHooks`, `MockClock`,
  `TestRandomSource`, `Scenario`, fixtures
- `src/module/*.rs` — per-module behavioral tests under `#[cfg(test)] mod tests`
- `tests/scenarios.rs` — integration scenarios using the `Scenario` DSL,
  which wraps a real `Daemon<TestPlatformHooks, MockApiClient>`

Run:

```sh
cargo test -p virtue-core                      # unit tests only
cargo test -p virtue-core --features testing   # unit + integration tests
```
