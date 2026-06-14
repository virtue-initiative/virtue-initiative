# CLAUDE.md — client/

Multi-platform Rust monitoring client. All platforms share `core/`; each platform crate
is a thin wrapper that supplies raw screen data and OS hooks.

## Where to find what

### Auth / login / logout

- `core/src/module/auth.rs` — `AuthModule`: handles `LoginRequested`/`LogoutRequested`,
  calls API, fires `Login`/`Logout` events with credentials, refreshes device settings on ping
- `core/src/storage.rs` — reads/writes `stop_intent.json` and other state files

### Event system

- `core/src/events/bus.rs` — `EventBus`, `Observer` trait, `Emitter`, `dispatch_event!` macro,
  `EventChannel` trait
- `core/src/events/types.rs` — all typed event structs (`Ping`, `Login`, `Upload`, `StatusRequest`, …)
- `core/src/events/remote.rs` — `RemoteEventBus` (cross-process JSON-line channel)
- `core/src/assembly.rs` — `build_default_modules()` factory for the 7 default observers

The daemon loop sends `Ping` events and calls `bus.iter()` on each cycle.
Observer state is persisted to `event_state.json` after each iteration.

### Upload / batching / hash chain

- `core/src/module/upload.rs` — `UploadModule`: manages 3 queues (hash immediate, batch,
  direct immediate), retry logic
- `core/src/module/upload/batch.rs` — `BatchBuilder`: msgpack + gzip batch construction
- `core/src/crypto.rs` — AES-256-GCM encryption, HPKE key wrap, `compute_event_hash`,
  `encode_batch_event`

### Screenshot capture

- `core/src/module/screenshot.rs` — `ScreenshotModule`: interval scheduling, fires
  `Upload { kind: Screenshot }` when authenticated
- `linux/src/capture.rs`, `mac/src/capture.rs`, `windows/src/capture.rs` — platform
  `take_screenshot()` implementations

### Lifecycle events / alerts

- `core/src/module/lifecycle.rs` — `LifecycleModule`: tracks process start/stop,
  suspend/resume, ping-gap detection, fires `Upload` on anomalies

### IPC (daemon ↔ controller)

- `core/src/ipc.rs` — `IpcListener` / `IpcSender` (Unix socket on Linux/Mac,
  in-process channel on Windows)
- `core/src/events/remote.rs` — `RemoteEventBus`: typed event channel over IPC; the
  daemon bridges CLI requests into the main `EventBus` via this
- `core/src/controller.rs` — IPC client used by CLI to query status, login, logout

### Status

- `core/src/module/status.rs` — `StatusModule`: collects `PartialStatus` fragments from
  3 modules, assembles and emits `StatusResponse`

### Platform daemons / main loops

- `linux/src/daemon.rs` — daemon loop, systemd suspend signals, IPC receiver threads
- `mac/src/daemon.rs` — daemon loop, IOKit power notifications, 30s post-wake suppression
- `windows/src/resident_monitor.rs` — in-process monitor, session/suspend events

### Configuration

- `core/src/config.rs` — `Config`: API base URL, screenshot/batch intervals, state dir
- `core/src/module/config.rs` — `ConfigModule`: hot-reloads `config_override.json` on Ping
- `linux/src/config.rs`, `mac/src/config.rs`, `windows/src/config.rs` — path discovery

### Testing

- `core/src/testing/` — `MockApiClient`, `TestPlatformHooks`, `MockClock`, `Scenario` DSL
- `core/src/module/*.rs` — per-module behavioral tests in `#[cfg(test)] mod tests`
- `core/tests/scenarios.rs` — integration-style scenario tests

## Open bugs

See `BUGS.md` in this directory.

## Key invariants (don't change without cross-component review)

See `../CLAUDE.md` (repo root) for wire format constraints shared with the TypeScript web app.
The most dangerous files are `core/src/crypto.rs` and `core/src/module/upload/batch.rs`.
