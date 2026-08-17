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
lock detection) and `LifecycleHooks` (the three hooks the late-wakeup model needs):

```rust
// ScreenshotHooks
take_screenshot() -> Result<Screenshot>
get_time_utc_ms() -> Result<i64>
is_locked_or_screensaver() -> Result<bool>

// LifecycleHooks
get_utc_clock_ms() -> Result<i64>
get_last_login_utc_ms() -> Result<Option<i64>>
get_last_logout_utc_ms() -> Result<Option<i64>>
```

`PlatformHooks: ScreenshotHooks + LifecycleHooks` is a blanket impl — platforms
never implement it directly. `get_boot_clock_ms`/`get_monotonic_clock_ms` were
removed from `LifecycleHooks` in the daemon rewrite (the late-wakeup model
doesn't use them); Mac keeps its own boot/monotonic clock reads as **inherent**
methods on `MacPlatformHooks` for a local post-wake UX check unrelated to the
core model — see `architecture.md`.

Everything else belongs in `core`.

## The daemon loop

`core` is a single sequential loop, not an event bus:

```rust
let daemon = Daemon::new(config, platform, api, state_path)?;
daemon.run_forever(); // blocking — call from its own thread
```

- `src/daemon.rs` — `Daemon<P, A>` / `DaemonState`; `tick_once` runs each
  phase (lifecycle check, screenshot plan/capture/commit, capture-availability,
  heartbeat, hash retries, batch upload, pick next wakeup, persist),
  releasing the state lock around slow work (capture, network I/O).
- `login`/`logout`/`status`/`note_user_stop`/`queue_upload`/`flush_batch_now`/
  `request_stop` are plain synchronous `Daemon` methods, protected by an
  `Arc<Mutex<DaemonState>>` + `Condvar` — no message queue. Each nudges the
  loop's next wakeup to "now" and notifies the condvar so `run_forever`'s
  sleep wakes promptly.
- `src/events/channel.rs` — `Event`/`EventChannel`/`Error`, used only by
  `RemoteEventBus` (IPC) now — there is no in-process event bus.
- `src/events/remote.rs` — `RemoteEventBus` (cross-process JSON-line channel)
- `src/ipc_bridge.rs` — `IpcBridge`: dispatches each accepted connection's
  inbound requests directly to `Daemon` methods.

## The 6 modules (`src/module/`)

Each is a plain `struct FooState` (serde default) plus free functions — no
trait, no event dispatch. A module that needs to enqueue work calls
`upload::enqueue(&mut upload_state, now_ms, risk, kind)` directly.

| Module                 | State                                                     | Key functions                                                                                                              |
| ---------------------- | --------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `auth`                 | (writes into `AuthState`/`ScreenshotState`/`UploadState`) | `login()`, `logout()`                                                                                                      |
| `lifecycle`            | `LifecycleState`                                          | `tick()` (late-wakeup check), `note_user_stop()`                                                                           |
| `screenshot`           | `ScreenshotState`                                         | `plan()`, `capture_and_process()`, `commit()`                                                                              |
| `upload`               | `UploadState`                                             | `enqueue()`, `plan_hash_retries`/`execute_hash_retries`/`commit_hash_retries`, `plan_batch`/`execute_batch`/`commit_batch` |
| `capture_availability` | `CaptureAvailabilityState`                                | `note_failure()`, `tick()`                                                                                                 |
| `heartbeat`            | `HeartbeatState`                                          | `tick()`                                                                                                                   |
| `status`               | —                                                         | `build()` (pure `ServiceStatus` assembly)                                                                                  |

`lifecycle::tick` compares actual vs. scheduled wakeup time each tick and
alerts on a single late wakeup > 1 min or a last-10-array sum > 5 min,
excused near a system login/logout — see `SPEC.md` §2 and `tampering.md`
(now a short pointer to SPEC.md, not its own model).

## State persistence

`event_state.json` (in `Config.state_dir`) holds the serialized `DaemonState`.
Top-level field names match the pre-rewrite per-observer keys (`auth`,
`lifecycle`, `screenshot`, `upload`, `capture_availability`, `heartbeat`) —
existing installs load cleanly. `Daemon::new` loads it (or defaults) and, if
already authenticated, refreshes device settings once before returning.

## IPC (Linux/Mac only)

- `src/events/remote.rs` — `RemoteEventBus`: typed event channel over a Unix
  socket
- `src/ipc_bridge.rs` — `IpcBridge`: accepts connections, wires each one
  directly to a shared `Arc<Daemon<...>>`
- `src/controller.rs` — `ClientController`: IPC client used by the CLI for
  login, logout, status. Its 6 public methods are a stable boundary — every
  platform crate depends on this exact surface.

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
