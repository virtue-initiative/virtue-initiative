# CLAUDE.md — client/

Multi-platform Rust monitoring client. All platforms share `core/`; each platform crate
is a thin wrapper that supplies raw screen data and OS hooks, then runs `core`'s
`Daemon::run_forever()` loop. `client/ios/rust` has its own standalone Cargo
workspace (not a `client/Cargo.toml` member) and its own CI job
(`.github/workflows/client-ios.yml`), which also handles TestFlight/App
Store releases — it isn't covered by workspace-wide `cargo check`/`test`
runs from `client/`. iOS disables the daemon's late-wakeup tamper check
(`IosPlatformHooks::lifecycle_enabled() -> false`) — the Safari extension
host has no boot/shutdown/session API surface, so there's no meaningful
"late wakeup" signal there — but otherwise runs the same daemon as every
other platform.

## Where to find what

### The daemon loop itself

- `core/src/daemon.rs` — `Daemon<P, A>` / `DaemonState`: one sequential loop.
  Each tick clones the current state once, runs it straight through (lifecycle
  check, maybe a screenshot, upload what's queued, pick the next wakeup) with
  no locking anywhere in the middle, then writes the result back. See
  `core/SPEC.md` and `core/architecture.md`.
- `login`/`logout`/`note_user_stop`/`queue_upload`/`flush_batch_now` are
  `Daemon` methods that send a request on an `mpsc` channel and block for the
  loop thread's reply — the loop thread is the only place `DaemonState` is
  ever mutated. `status()` is a direct lock-based read (nothing to
  synchronize); `request_stop()` is fire-and-forget.

### Auth / login / logout

- `core/src/module/auth.rs` — `login()`/`logout()`: calls the API, updates
  `AuthState`/`ScreenshotState`/`UploadState` directly (settings/hash token are
  subsequently refreshed opportunistically from every `GET /d/device` and
  `POST /d/batch` response, not a dedicated pre-batch fetch)

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
  random exponential-cadence scheduling (CORE-003 — not a fixed interval),
  lock/screensaver gate, screen-change diff gate, redaction, risk classification.
  `plan_forced()` is the on-demand "capture now" entry point (wired through
  `Daemon::force_capture_now`): bypasses the interval-due gate but keeps the
  locked/screensaver gate, then reuses `capture_and_process`/`commit` unchanged.
- `linux/src/capture.rs`, `mac/src/capture.rs`, `windows/src/capture.rs` — platform
  `take_screenshot()` implementations

### Lifecycle / tamper alerts

- `core/src/module/lifecycle.rs` — `tick()`: compares actual vs. scheduled
  wakeup time each tick and alerts (`UploadKind::ScreenshotMissed`) on a single
  late wakeup > 1 min or a last-10-array sum > 5 min, excused near a system
  login/logout; `note_user_stop()` — immediate high-risk alert, unrelated to
  the late-wakeup check. See CORE-002; `core/tampering.md` is now
  just a pointer there (a richer suspend/reboot/gap-bucket model was retired
  in the daemon rewrite).

### IPC (daemon ↔ controller, Linux/Mac only)

- `core/src/ipc.rs` — single file. Windows/Android/iOS need nothing here;
  they already reach `Daemon` in-process. Linux/Mac's CLI/tray runs as a
  separate process, so `spawn_server` binds a Unix socket and serves it on
  one thread (`accept` -> serve one connection to completion -> `accept`
  again — only one client at a time), translating newline-JSON
  `WireRequest`s directly to `Daemon` method calls. `ClientController` is
  the client side — a stable boundary every Linux/Mac platform crate
  depends on, reached as `virtue_core::ipc::ClientController` (not
  re-exported at the crate root, since the module doesn't exist on every
  platform). The module carries its own `#![cfg]`, so `target_os` appears
  in this one file and nowhere else in `core`.

### Status

- `core/src/module/status.rs` — `build()`: pure `ServiceStatus` assembly
  from `&AuthState`/`&UploadState` — no fan-in, since everything lives in
  one `DaemonState` now

### Platform daemons / main loops

- `linux/src/daemon.rs` — spawns the IPC server thread (`ipc::spawn_server`)
  and `Daemon::run_forever()` on its own thread, then joins the latter
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
- `ios/rust/src/lib.rs` — same C-FFI/`Arc<Daemon>` shape as Android
  (`virtue_ios_native_init` builds one daemon; every other
  `virtue_ios_native_*` call is a direct method call on it);
  `IosPlatformHooks::lifecycle_enabled()` returns `false`, so
  `lifecycle::tick` never runs there

### Configuration

- `core/src/config.rs` — `Config`: API base URL, screenshot/batch intervals, state dir.
  All three values are compile-time constants baked in via `env!()` through `core/build.rs`,
  which also loads the repo-root `.env` (gitignored; see `.env.example`) and, beneath that,
  `~/.config/virtue-dev.env` (see root `AGENTS.md`) for local compile-time defaults — there
  is no runtime override mechanism.
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
