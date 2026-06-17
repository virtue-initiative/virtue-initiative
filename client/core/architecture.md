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
      ipc_bridge.rs     — IpcBridge (Linux/Mac daemon IPC accept loop)
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

### Screenshot dedup (two gates)

`ScreenshotModule` still captures on the normal cadence, but a frame is only _uploaded_
when it carries new information. Two gates run on each due `Ping`:

1. **Lock / screensaver gate** — checked _before_ capturing via the
   `is_locked_or_screensaver()` platform hook. While the session is locked or a screensaver
   is active the user cannot be viewing real content, so the capture is skipped entirely (no
   `take_screenshot`, no classification, no `Upload`). The cadence clock still advances so we
   re-check next interval. The hook fails safe to `false` (fall back to the diff gate) when
   the state is unknown. Per platform: Linux uses `org.freedesktop.ScreenSaver` /
   `org.gnome.ScreenSaver` `GetActive()` over D-Bus; macOS reads `CGSSessionScreenIsLocked`
   and detects the screensaver process; Windows uses `SPI_GETSCREENSAVERRUNNING` plus an
   `OpenInputDesktop` lock check.

2. **Screen-change diff gate** — after capturing, the frame is reduced to a grayscale grid
   fingerprint (`module/screenshot/fingerprint.rs`). If it has not materially changed from the
   **last uploaded** fingerprint, the upload is suppressed. A frame counts as changed if either
   the **mean** per-cell delta is high (a broad change, including low-contrast ones such as a
   terminal filling with text) OR a **number of cells** changed strongly (a concentrated change
   such as a video window — the count, not a single max, ignores a 1–2 cell clock/cursor while
   still catching a small corner video, which is what makes the gate abuse-resistant). The grid
   resolution is **derived from the image size** (each cell ≈ a fixed source-pixel block, aspect
   preserved); a fixed grid would make each cell average thousands of pixels on a large/wide
   display and dilute real changes below threshold. Anchoring to the last _uploaded_ frame — not
   the previous capture — means slow sub-threshold drift eventually accumulates past the
   threshold and forces a fresh upload. `last_uploaded_fingerprint` is persisted in
   `event_state.json`.

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

- **`IpcListener` / IPC transport** (`events/remote.rs`) — low-level Unix-socket;
  provides raw JSON-line send/recv via `RemoteEventBus`.
- **`RemoteEventBus`** (`events/remote.rs`) — typed JSON-line event channel;
  implements `EventChannel` so `ClientController` works against both in-process
  and cross-process peers.

The daemon binds an `IpcListener`, wraps each accepted connection in a
`RemoteEventBus`, and bridges it to the main `EventBus` by forwarding typed
events in both directions. Non-forwarded types (`Upload`, `Ping`) never cross
the socket boundary.

#### IpcBridge (`ipc_bridge.rs`)

`IpcBridge` encapsulates the boilerplate shared by the Linux and Mac daemons:
the accept thread, the list of connected outbound senders, and the fan-out
subscription. Typical usage:

```rust
// Once, before the main loop:
let mut ipc = IpcBridge::bind(&paths.state_dir.join("daemon.sock"));
if let Some(ipc) = &mut ipc {
    ipc.subscribe_standard_outbound(&mut bus);         // LoginResult, LogoutResult, …
    ipc.subscribe_outbound::<MyPlatformEvent>(&mut bus); // platform-specific extras
}

// Each main-loop iteration:
if let Some(ipc) = &mut ipc {
    ipc.accept_pending(&mut bus, IpcBridge::forward_standard_inbound);
    // or a custom setup closure for platforms that need extra per-connection handlers
}
```

`forward_standard_inbound` registers handlers for the standard controller→daemon
set (`LoginRequested`, `LogoutRequested`, `StatusRequest`, `UserStopRequested`,
`UserSessionLogin/Logout`, `ComputerSuspended/Resumed`, `ProcessStopped`).
Platform daemons can pass a custom closure to `accept_pending` to add extra
handlers per-connection (Mac uses this to track `UserStopRequested` separately
for shutdown-reason classification).

## Platform process model

Each platform uses a different integration pattern:

### Linux / Mac — separate daemon process

The daemon runs as a separate process. Communication between the CLI/tray and the
daemon uses Unix-domain sockets (`daemon.sock`) via `IpcBridge`. The main event
loop sends `Ping` on each tick and calls `ipc.accept_pending()` to wire up newly
connected controllers.

### Windows — in-process `ResidentMonitor` thread

There is no separate daemon process. `ResidentMonitor` runs the event bus on a
background thread inside the host process. The CLI communicates via an in-process
`mpsc` channel rather than a Unix socket.

### Android / iOS — JNI/C FFI entry points

Each JNI or C FFI call builds a fresh `EventBus`, sends one or more events, calls
`bus.iter()`, persists the resulting state, and returns. There is no long-running
daemon loop from the host language's perspective; the Rust code holds no
cross-call state beyond what is serialised to `event_state.json`. No IPC
sockets are used.

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
fn get_time_utc_ms(&self) -> CoreResult<i64>;            // default: SystemTime::now()
fn get_last_shutdown_time_utc_ms(&self) -> CoreResult<Option<i64>>;
fn get_last_startup_time_utc_ms(&self) -> CoreResult<Option<i64>>;
fn is_locked_or_screensaver(&self) -> CoreResult<bool>;  // default: Ok(false)
```

`get_time_utc_ms` has a default implementation using `SystemTime::now()` that is correct for
all production platforms. Only `TestPlatformHooks` overrides it (delegates to `MockClock` for
time-controlled tests). `is_locked_or_screensaver` defaults to `Ok(false)` (the fail-safe);
desktop platforms override it for the screenshot dedup lock/screensaver gate, while mobile
platforms keep the default. Platform crates implement `take_screenshot`,
`get_last_shutdown_time_utc_ms`, `get_last_startup_time_utc_ms`, and (on desktop)
`is_locked_or_screensaver`.

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
