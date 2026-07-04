# CLAUDE.md — client/core

Shared Rust library used by all platform clients. **Read `architecture.md` before making non-trivial changes.**

## Design rule

Platform crates provide only raw screen data. `core` owns everything else: request flow, persistence, retrying, hashing, batch construction, compression, encryption, and upload.

## Most dangerous files to edit

These files implement the client side of cross-component contracts shared with the TypeScript web app. Changing them incorrectly causes silent data corruption or decryption failures:

- `src/crypto.rs` — AES-256-GCM encryption, HPKE key wrapping, content hash computation
- `src/module/upload/batch.rs` — msgpack + gzip batch payload construction

See `../CLAUDE.md` (repo root) for the exact wire formats and constraints.

## PlatformHooks

Keep the trait minimal. Platforms implement only:

```rust
take_screenshot() -> Result<Screenshot>
get_time_utc_ms() -> Result<i64>
get_last_shutdown_time_utc_ms() -> Result<Option<i64>>
get_last_startup_time_utc_ms() -> Result<Option<i64>>
```

Everything else belongs in `core`.

## Event system

`core` uses a typed in-process event bus:

- `src/events/bus.rs` — `EventBus`, `Observer` trait, `Emitter`, `dispatch_event!` macro, `EventChannel` trait
- `src/events/types.rs` — all typed event structs (`Ping`, `Login`, `Upload`, `StatusRequest`, …)
- `src/events/remote.rs` — `RemoteEventBus` (cross-process JSON-line channel)
- `src/assembly.rs` — `build_default_modules()` factory for the 7 default observers

The daemon loop is:

```rust
let observers = build_default_modules(config, platform, api, PlatformConfig::default())?;
let mut bus = EventBus::new(observers, saved_state)?;
loop {
    bus.send(Ping)?;
    let state = bus.iter()?;
    store_state(&state_path, &state)?;
    // sleep until next interval
}
```

## The 7 observer modules (`src/module/`)

| Module                 | Handles                                                                       | Emits                                                                   |
| ---------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `auth`                 | `LoginRequested`, `LogoutRequested`, `StatusRequest`                          | `Login`, `Logout`, `LoginResult`, `LogoutResult`, `PartialStatus::Auth` |
| `lifecycle`            | `Ping`, `ProcessStarted/Stopped`, `ComputerSuspended/Resumed`, `UserSession*` | `Upload` (lifecycle + alerts)                                           |
| `screenshot`           | `Login`, `Logout`, `Ping`, `ConfigChanged`                                    | `Upload` (screenshot), `CaptureFailed`                                  |
| `upload`               | `Login`, `Logout`, `Upload`, `Ping`, `ProcessStopped`, `FlushBatchNow`        | network I/O                                                             |
| `capture_availability` | `CaptureFailed`                                                               | `Upload` (capture-failed alert)                                         |
| `status`               | `StatusRequest`, `PartialStatus`                                              | `StatusResponse`                                                        |
| `config`               | `Ping`                                                                        | `ConfigChanged`                                                         |

## State persistence

`event_state.json` (in `Config.state_dir`) holds the serialised state of every observer.
The bus key is `Observer::name()`. Reload it on startup:

```rust
let state = load_state(&state_path).unwrap_or(StateType::Null);
let bus = EventBus::new(observers, state)?;
```

## IPC

- `src/ipc.rs` — `IpcListener` / `IpcSender` (Unix socket on Linux/Mac, in-process on Windows)
- `src/events/remote.rs` — `RemoteEventBus`: typed event channel over IPC; used by the daemon to bridge CLI requests into the main bus
- `src/controller.rs` — `ClientController`: IPC client used by CLI for login, logout, status

## Testing

- `src/testing/` — `MockApiClient`, `TestPlatformHooks`, `MockClock`, `Scenario`, fixtures
- `src/module/*.rs` — per-module behavioral tests under `#[cfg(test)] mod tests`
- `tests/scenarios.rs` — integration scenarios using the `Scenario` DSL

Run:

```sh
cargo test -p virtue-core                      # unit tests only
cargo test -p virtue-core --features testing   # unit + integration tests
```
