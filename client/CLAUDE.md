# CLAUDE.md — client/

Multi-platform Rust monitoring client. All platforms share `core/`; each platform crate
is a thin wrapper that supplies raw screen data and OS hooks.

## Where to find what

### Auth / login / logout

- `core/src/module/auth.rs` — `AuthModule`: handles `LoginRequested`/`LogoutRequested`,
  calls API, fires `Login`/`Logout` events with credentials and initial device settings
  (settings are subsequently refreshed by `UploadModule` before each batch upload)
- `core/src/storage.rs` — reads/writes `stop_intent.json` and other state files

### Event system

- `core/src/events/bus.rs` — `EventBus`, `Observer` trait, `Emitter`, `dispatch_event!` macro,
  `EventChannel` trait
- `core/src/events.rs` — the `Ping` event struct; other typed events live inline in the
  module file that owns them (`Login`/`Logout` in `module/auth.rs`, `ProcessStarted`/
  `SystemLoginObserved`/etc. in `module/lifecycle.rs`, `StatusRequest` in `module/status.rs`)
- `core/src/events/remote.rs` — `RemoteEventBus` (cross-process JSON-line channel)
- `core/src/assembly.rs` — `build_default_modules()` factory for the 8 default observers

The daemon loop sends `Ping` events and calls `bus.iter()` on each cycle.
Observer state is persisted to `event_state.json` after each iteration.

### Upload / batching / hash chain

- `core/src/module/upload.rs` — `UploadModule`: manages 3 queues (hash-pending, batch-pending,
  notify-pending), retry logic
- `core/src/module/upload/batch.rs` — `BatchBuilder`: msgpack + gzip batch construction
- `core/src/crypto.rs` — AES-256-GCM encryption, HPKE key wrap, `compute_event_hash`,
  `encode_batch_event`

### Screenshot capture

- `core/src/module/screenshot.rs` — `ScreenshotModule`: interval scheduling, fires
  `Upload { kind: Screenshot }` when authenticated
- `linux/src/capture.rs`, `mac/src/capture.rs`, `windows/src/capture.rs` — platform
  `take_screenshot()` implementations

### Lifecycle events / alerts

- `core/src/module/lifecycle.rs` — `LifecycleModule`: detects gaps in the expected
  login→logout running window (mid-session, at-start, at-stop), deriving suspend
  from boot-vs-monotonic clock divergence rather than OS sleep/wake events; fires
  `Upload` on anomalies. See `core/tampering.md` for the full model.

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

- `linux/src/daemon.rs` — daemon loop, IPC receiver threads
- `mac/src/daemon.rs` — daemon loop, NSWorkspace shutdown notification, local
  boot-vs-monotonic divergence check driving 30s post-wake suppression
- `windows/src/resident_monitor.rs` — in-process monitor, session-end (`WM_ENDSESSION`) events

Suspend/resume is no longer driven by real-time OS notifications on any
platform — `LifecycleModule` derives it from `boot_clock − monotonic_clock`
divergence sampled each `Ping` instead.

### Configuration

- `core/src/config.rs` — `Config`: API base URL, screenshot/batch intervals, state dir
- `core/src/module/config.rs` — `ConfigModule`: hot-reloads `config_override.json` on Ping
- `linux/src/config.rs`, `mac/src/config.rs`, `windows/src/config.rs` — path discovery

### Testing

- `core/src/testing/` — `MockApiClient`, `TestPlatformHooks`, `MockClock`, `Scenario` DSL
- `core/src/module/*.rs` — per-module behavioral tests in `#[cfg(test)] mod tests`
- `core/tests/scenarios.rs` — integration-style scenario tests

## Key invariants (don't change without cross-component review)

See `../CLAUDE.md` (repo root) for wire format constraints shared with the TypeScript web app.
The most dangerous files are `core/src/crypto.rs` and `core/src/module/upload/batch.rs`.
