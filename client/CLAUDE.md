# CLAUDE.md — client/

Multi-platform Rust monitoring client. All platforms share `core/`; each platform crate
is a thin wrapper that supplies raw screen data and OS hooks.

## Where to find what

### Auth / login / logout

- `core/src/module/auth.rs` — `AuthObserver`: handles `LoginRequested`/`LogoutRequested`,
  calls API, fires `Login`/`Logout` events with credentials, refreshes device settings on ping
- `core/src/auth.rs` — `Auth` struct: in-memory credential state, `persist()`/`load()`
- `core/src/storage.rs` — reads/writes `auth.json`, `device_settings.json`, `stop_intent.json`

### Event system

- `core/src/events.rs` — `Event` enum (all variants), `Observer` trait, `EventLoop`
  (dispatch loop + state persist via `event_state.json`)
- `core/src/service.rs` — `MonitorService`: assembles the 7 observers, runs `loop_iteration`,
  exposes IPC binding and typed observer accessors

### Upload / batching / hash chain

- `core/src/module/upload.rs` — `UploadObserver`: manages 3 queues (hash, immediate, batch),
  retry logic with backoff, 400/404 handling
- `core/src/module/upload/batch.rs` — `BatchBuilder`: msgpack + gzip batch construction
- `core/src/crypto.rs` — AES-256-GCM encryption, HPKE key wrap, `compute_event_hash`,
  `encode_batch_event`

### Screenshot capture

- `core/src/module/screenshot.rs` — `ScreenshotObserver`: interval scheduling, fires
  `Upload { kind: Screenshot }` when authenticated
- `linux/src/capture.rs`, `mac/src/capture.rs`, `windows/src/capture.rs` — platform
  `take_screenshot()` implementations

### Lifecycle events / alerts

- `core/src/module/lifecycle.rs` — `LifecycleObserver`: tracks process start/stop,
  suspend/resume, missed-shutdown backfill, fires `LifecycleAlert` on anomalies

### IPC (daemon ↔ controller)

- `core/src/ipc.rs` — `IpcListener` / `IpcSender` (Unix socket on Linux/Mac,
  in-process channel on Windows), event serialization
- `core/src/module/request_handler.rs` — `RequestObserver`: broadcasts events to
  connected controllers, filters `Login`/`Upload`/`Ping`
- `core/src/controller.rs` — IPC client used by CLI to query status, login, logout

### Status

- `core/src/module/status.rs` — `StatusObserver`: maintains `ServiceStatus` snapshot,
  responds to `StatusRequest` with `StatusResponse`

### Platform daemons / main loops

- `linux/src/daemon.rs` — daemon loop, systemd suspend signals, IPC receiver threads
- `mac/src/daemon.rs` — daemon loop, IOKit power notifications, 30s post-wake suppression
- `windows/src/resident_monitor.rs` — in-process monitor (no separate daemon process),
  session/suspend events, thread-safe status snapshot

### Configuration

- `core/src/config.rs` — `Config`: API base URL, screenshot/batch intervals, state dir
- `linux/src/config.rs`, `mac/src/config.rs`, `windows/src/config.rs` — path discovery
- Runtime override file: `config_override.json` in state dir (hot-reloaded each iteration)

### Testing

- `core/src/testing/` — mock `PlatformHooks`, scenario helpers
- `core/tests/scenarios.rs` — integration-style scenario tests

## Open bugs

See `BUGS.md` in this directory.

## Key invariants (don't change without cross-component review)

See `../CLAUDE.md` (repo root) for wire format constraints shared with the TypeScript web app.
The most dangerous files are `core/src/crypto.rs` and `core/src/module/upload/batch.rs`.
