# Core Architecture

Shared Rust library used by all platform clients.

## Design rule

Platform crates provide only raw screen data. `core` owns everything else:
request flow, persistence, retrying, hashing, batch construction, compression,
encryption, and upload semantics.

## Workspace layout

```
client/
  core/
    architecture.md
    Cargo.toml
    src/
      api.rs            — ApiTransport trait + ReqwestApiClient
      assembly.rs       — build_default_modules() factory
      config.rs         — Config struct + runtime override file
      controller.rs     — ClientController (IPC client for CLI)
      crypto.rs         — AES-256-GCM, HPKE key wrap, hash computation
      error.rs          — CoreError / CoreResult
      events/
        bus.rs          — EventBus, Observer, Emitter, dispatch_event!
        remote.rs       — RemoteEventBus (cross-process, JSON lines)
        types.rs        — 30+ typed event structs
      ipc.rs            — Unix / in-process IPC primitives
      model.rs          — Shared structs (ServiceStatus, Screenshot, …)
      module/
        auth.rs         — AuthModule: login / logout / device settings
        capture_availability.rs — CaptureAvailabilityModule: failure threshold
        config.rs       — ConfigModule: runtime override file hot-reload
        lifecycle.rs    — LifecycleModule: process/suspend/ping-gap alerts
        screenshot.rs   — ScreenshotModule: interval scheduling + capture
        status.rs       — StatusModule: partial-status aggregation
        upload.rs       — UploadModule: hash/batch/immediate queues
      platform.rs       — ScreenshotHooks / PlatformHooks traits
      state.rs          — load_state / store_state (event_state.json)
      storage.rs        — auth.json, device_settings.json, stop_intent.json
      testing/          — MockApiClient, TestPlatformHooks, MockClock, Scenario
  linux/  mac/  windows/  android/  ios/   — platform wrappers
```

## Event bus model

`core` is structured around a typed, in-process event bus.

```rust
// Build the default set of 7 observer modules.
let observers = build_default_modules(config, platform, api)?;
let mut bus = EventBus::new(observers, saved_state)?;

// One loop iteration: send Ping, process all cascaded events.
bus.send(Ping)?;
let state = bus.iter()?;   // returns serialisable snapshot of all observer state

// Request/response status check.
let resp: StatusResponse = bus.request(StatusRequest)?;
```

### Observer trait

Each module implements `Observer`:

```rust
pub trait Observer: 'static {
    fn init(&mut self, bus: &mut EventBus, state: StateType) -> CoreResult<()>;
    fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()>;
    fn save(&self) -> CoreResult<StateType>;
    fn name(&self) -> &'static str;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
```

- **`init`** — called once at bus construction; restore saved state here and
  register subscription closures on `bus` if needed.
- **`on_event`** — called for every event in the queue; emit follow-up events
  via `emitter.send(...)`.
- **`save`** — snapshot durable state; the bus aggregates these into a JSON
  object keyed by `Observer::name`.
- **`as_any_mut`** — enables test helpers to `downcast_mut` to the concrete type.

Use `crate::dispatch_event!(event, { pat: Type => expr, … })` inside `on_event`
to pattern-match typed events without boilerplate.

### The 7 default modules

| Module                      | Key inputs                                                                              | Key outputs                                                                                        |
| --------------------------- | --------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `LifecycleModule`           | `Ping`, `ProcessStarted/Stopped`, `ComputerSuspended/Resumed`, `UserSession*`           | `Upload` (lifecycle + alert events)                                                                |
| `ScreenshotModule`          | `Login`, `Logout`, `Ping`, `ConfigChanged`                                              | `Upload` (screenshot), `CaptureFailed`                                                             |
| `UploadModule`              | `Login`, `Logout`, `Upload`, `Ping`, `ProcessStopped`, `FlushBatchNow`, `ConfigChanged` | network I/O via `ApiTransport`; `LogoutRequested` on 404                                           |
| `CaptureAvailabilityModule` | `CaptureFailed`                                                                         | `Upload` (capture-failed alert)                                                                    |
| `AuthModule`                | `LoginRequested`, `LogoutRequested`, `Ping`, `StatusRequest`, `ConfigChanged`           | `Login`, `Logout`, `LoginResult`, `LogoutResult`, `DeviceSettingsRefreshed`, `PartialStatus::Auth` |
| `StatusModule`              | `StatusRequest`, `PartialStatus` (from 3 sources)                                       | `StatusResponse`                                                                                   |
| `ConfigModule`              | `Ping`                                                                                  | `ConfigChanged`                                                                                    |

### Request/response status flow

`StatusRequest` triggers the three modules that contribute to service status
(`LifecycleModule`, `AuthModule`, `UploadModule`) to each emit one
`PartialStatus` fragment. `StatusModule` collects all three and emits a single
`StatusResponse`. The `EventBus::request` helper handles this synchronously:

```rust
let resp: StatusResponse = bus.request(StatusRequest)?;
assert!(resp.status.is_authenticated);
```

### State persistence

Each `bus.iter()` call returns the aggregated state snapshot. The caller is
responsible for persisting it (e.g. to `event_state.json`) and reloading on the
next startup:

```rust
let state = load_state(&state_path).unwrap_or(StateType::Null);
let bus = EventBus::new(observers, state)?;
```

### IPC: relay and transport split

Cross-process communication uses two layers:

- **`IpcListener` / IPC transport** (`ipc.rs`) — low-level Unix-socket or
  in-process channel; provides raw line send/recv.
- **`RemoteEventBus`** (`events/remote.rs`) — typed JSON-line event channel
  built on top of the transport; implements `EventChannel` so `ClientController`
  works against both in-process and cross-process peers.

The daemon binds an `IpcListener`, wraps it in a `RemoteEventBus`, and bridges
it to the main `EventBus` by subscribing to forwarded event types (e.g.
`StatusRequest`, `LoginRequested`, `LogoutRequested`). Non-forwarded types
(`Upload`, `Ping`) never cross the socket boundary.

## Config model

`Config` fields:

- `api_base_url` — REST API base URL
- `device_name` — stable device identifier
- `platform_name` — e.g. `"linux"`, `"mac"`, `"windows"`
- `state_dir` — directory for all state files
- `runtime_config_file` — optional path to `config_override.json` (hot-reloaded)
- `screenshot_interval` — default 60 s
- `batch_interval` — default 60 s

Override keys supported in `config_override.json`:

```json
{
  "api_base_url": "https://...",
  "capture_interval_seconds": 30,
  "batch_window_seconds": 120
}
```

`ConfigModule` re-reads this file on every `Ping` and emits `ConfigChanged`
only when a value actually changes.

## State files (under `Config.state_dir`)

| File               | Owner          | Purpose                                                                               |
| ------------------ | -------------- | ------------------------------------------------------------------------------------- |
| `event_state.json` | `EventBus`     | Serialised observer state (screenshot schedule, upload queues, lifecycle state, auth) |
| `errors.log`       | `UploadModule` | Permanent failures (400 responses); append-only                                       |

Auth and device settings are now stored inside `event_state.json` under the
`auth` and `upload` keys respectively (they were previously separate files, but
are now owned entirely by their observer modules).

## PlatformHooks

Keep the trait minimal. Platforms implement only:

```rust
fn take_screenshot(&self) -> CoreResult<Screenshot>;
fn get_time_utc_ms(&self) -> CoreResult<i64>;
fn get_last_shutdown_time_utc_ms(&self) -> CoreResult<Option<i64>>;
fn get_last_startup_time_utc_ms(&self) -> CoreResult<Option<i64>>;
```

Everything else belongs in `core`.

## Batch blob format

See `../CLAUDE.md` (repo root) for the exact wire format. Summary:

```
events → encode_batch_event() per event → BatchBuilder::build_upload()
       → msgpack({events: [...]}) → gzip → AES-256-GCM
wire:  nonce[12 bytes] || ciphertext+tag
```

Each upload also wraps the batch key per recipient using HPKE
(`DhkemX25519HkdfSha256 / HkdfSha256 / Aes256Gcm`).

## Hash chain

Per-event content hashes are uploaded to `POST /hash` independently of batches:

```
content_hash = sha256(ts_le64 || type_utf8 || sorted(key_utf8 || encoded_value))
new_state    = sha256(current_state[32] || content_hash[32])
```

## Testing

The `testing` feature (auto-enabled under `cfg(test)`) exposes:

- `MockApiClient` — records calls, serves canned responses
- `TestPlatformHooks` / `MockClock` — controllable time, queued screenshots
- `Scenario` — full 7-module bus with helper methods for time control,
  state seeding, and API assertions
- `fixtures` — minimal valid PNG for unit tests

Integration tests live in `core/tests/scenarios.rs` and use `Scenario`.
Per-module behavioral tests live in each module file under `#[cfg(test)] mod tests`.
