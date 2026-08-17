# CLAUDE.md — client/

Multi-platform Rust monitoring client. All platforms share `core/`; each platform crate
is a thin wrapper that supplies raw screen data and OS hooks, then runs `core`'s
`Daemon::run_forever()` loop. `client/ios/` is excluded from the Cargo workspace —
screenshot capture there is only possible while the Safari extension host is
actively running, which doesn't fit this daemon's background-monitoring model.

## Where to find what

### The daemon loop itself

- `core/src/daemon.rs` — `Daemon<P, A>` / `DaemonState`: one sequential loop
  (`tick_once`) — check lifecycle, maybe take a screenshot, upload what's
  queued, pick the next wakeup, sleep. See `core/SPEC.md` and
  `core/architecture.md`.
- `login`/`logout`/`status`/`note_user_stop`/`queue_upload`/`flush_batch_now`/
  `request_stop` are plain synchronous `Daemon` methods, protected by an
  `Arc<Mutex<DaemonState>>` + `Condvar` — not an event bus or message queue.

### Auth / login / logout

- `core/src/module/auth.rs` — `login()`/`logout()`: calls the API, updates
  `AuthState`/`ScreenshotState`/`UploadState` directly (settings/hash token are
  subsequently refreshed opportunistically from every `GET /d/device` and
  `POST /d/batch` response, not a dedicated pre-batch fetch); `LoginRequested`/
  `LoginResult`/`Logout`/`LogoutRequested`/`LogoutResult` are the IPC wire
  message types (`Logout` is a daemon-initiated push, sent whenever the
  daemon transitions to logged-out)

### Upload / batching / hash chain

- `core/src/module/upload.rs` — `UploadState`: two queues (hash-pending,
  batch-pending) plus persisted exponential backoff (`RetryBackoff`, survives
  a daemon restart); `plan_*`/`execute_*`/`commit_*` split so network I/O runs
  without holding the state lock. High-risk notify metadata rides with its
  event into whichever batch carries it rather than living in a separate queue.
- `core/src/module/upload/batch.rs` — `BatchBuilder`: msgpack + gzip batch construction
- `core/src/crypto.rs` — AES-256-GCM encryption, HPKE key wrap, `compute_event_hash`,
  `encode_batch_event`

### Screenshot capture

- `core/src/module/screenshot.rs` — `plan()`/`capture_and_process()`/`commit()`:
  random exponential-cadence scheduling (SPEC.md §3 — not a fixed interval),
  lock/screensaver gate, screen-change diff gate, redaction, risk classification
- `linux/src/capture.rs`, `mac/src/capture.rs`, `windows/src/capture.rs` — platform
  `take_screenshot()` implementations

### Lifecycle / tamper alerts

- `core/src/module/lifecycle.rs` — `tick()`: compares actual vs. scheduled
  wakeup time each tick and alerts (`AlertReason::LateWakeup`) on a single
  late wakeup > 1 min or a last-10-array sum > 5 min, excused near a system
  login/logout; `note_user_stop()` — immediate high-risk alert, unrelated to
  the late-wakeup check. See `core/SPEC.md` §2; `core/tampering.md` is now
  just a pointer there (a richer suspend/reboot/gap-bucket model was retired
  in the daemon rewrite).

### IPC (daemon ↔ controller, Linux/Mac only)

- `core/src/events/remote.rs` — `RemoteEventBus`: typed JSON-line event
  channel over a Unix socket
- `core/src/ipc_bridge.rs` — `IpcBridge`: accepts connections and dispatches
  each one's inbound requests **directly to `Daemon` methods** (no
  in-process bus to bridge into)
- `core/src/controller.rs` — `ClientController`: IPC client used by the CLI
  to query status, login, logout — a stable 6-method boundary every
  platform depends on

### Status

- `core/src/module/status.rs` — `build()`: pure `ServiceStatus` assembly
  from `&AuthState`/`&UploadState` — no fan-in, since everything lives in
  one `DaemonState` now

### Platform daemons / main loops

- `linux/src/daemon.rs` — spawns `Daemon::run_forever()` on its own thread;
  main thread polls `IpcBridge::accept_pending`
- `mac/src/daemon.rs` — same, plus a local boot-vs-monotonic divergence poll
  (via `MacPlatformHooks`'s inherent `boot_clock_ms`/`monotonic_clock_ms`
  methods, not part of `LifecycleHooks`) driving a post-wake
  `daemon.flush_batch_now()` — independent daemon-loop UX plumbing, not part
  of the core alerting model
- `windows/src/resident_monitor.rs` — builds one `Arc<Daemon>` in
  process-global state, spawns `run_forever()` on a background thread;
  `app_login`/`app_logout`/`status_snapshot`/stop functions call its
  synchronous methods directly
- `android/rust/src/lib.rs` — `nativeInit` builds one `Arc<Daemon>`; every
  other `native*` JNI entry point calls a method on it directly
  (`nativeRunDaemonLoop` → `run_forever()`, `nativeStopDaemon` →
  `request_stop()`)

### Configuration

- `core/src/config.rs` — `Config`: API base URL, screenshot/batch intervals, state dir.
  All three values are compile-time constants baked in via `env!()` through
  `core/build.rs`, which also loads an optional `client/.env` file (gitignored;
  see `client/.env.example`) — there is no runtime override mechanism.
- `linux/src/config.rs`, `mac/src/config.rs`, `windows/src/config.rs` — path discovery

### Testing

- `core/src/testing/` — `MockApiClient`, `TestPlatformHooks`, `MockClock`,
  `TestRandomSource`, `Scenario` (wraps a real `Daemon<TestPlatformHooks,
MockApiClient>` built via the same `Daemon::new` production uses)
- `core/src/module/*.rs` — per-module behavioral tests in `#[cfg(test)] mod tests`
- `core/tests/scenarios.rs` — integration-style scenario tests

## Key invariants (don't change without cross-component review)

See `../CLAUDE.md` (repo root) for wire format constraints shared with the TypeScript web app.
The most dangerous files are `core/src/crypto.rs` and `core/src/module/upload/batch.rs`.
