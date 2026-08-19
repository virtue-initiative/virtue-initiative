# Core Architecture

Shared Rust library used by all platform clients.

## Design rule

Platform crates provide only raw screen data and OS hooks. `core` owns
everything else: the daemon loop, persistence, retrying, hashing, batch
construction, compression, encryption, and upload semantics.

## Workspace layout

```
client/
  core/
    architecture.md
    SPEC.md             — source of truth for the daemon loop's behavior
    Cargo.toml
    src/
      api.rs            — ApiTransport trait + HttpApiClient
      config.rs         — Config struct (compile-time defaults via env!())
      crypto.rs         — AES-256-GCM, HPKE key wrap, hash computation
      daemon.rs         — Daemon<P, A> / DaemonState — the sequential loop itself
      error.rs          — CoreError / CoreResult
      ipc.rs            — ClientController + cross-process Unix-socket transport
                           (Linux/Mac only)
      model.rs          — Shared structs (ServiceStatus, Screenshot, …)
      module/
        auth.rs                  — login()/logout()
        capture_availability.rs  — failure-window threshold
        heartbeat.rs             — 24h liveness ping
        lifecycle.rs             — late-wakeup tamper check + UserStop
        screenshot.rs            — plan/capture/commit + dedup + redaction
        status.rs                — pure ServiceStatus assembly
        upload.rs                — hash/batch queues, backoff, plan/execute/commit
      platform.rs       — ScreenshotHooks / LifecycleHooks / PlatformHooks traits
      rng.rs            — RandomSource (screenshot cadence draws)
      state.rs          — load_state / store_state (event_state.json)
      testing/          — MockApiClient, TestPlatformHooks, MockClock,
                           TestRandomSource, Scenario
  linux/  mac/  windows/  android/  ios/   — platform wrappers (ios has its
                                              own standalone Cargo workspace,
                                              not a member of client/'s — see
                                              below)
```

## The daemon loop

`core` is structured around **one sequential loop**, not an event bus.
`Daemon<P: PlatformHooks, A: ApiTransport>` owns an `Arc<Mutex<DaemonState>>`
(a read-only snapshot outside the loop thread) and an `mpsc::Sender<DaemonRequest>`.
Each iteration of `run_forever` clones the state once, then runs the phases
below straight through against that owned clone — no locking anywhere in the
middle — before writing the result back to the shared snapshot and to disk:

```
run_forever loop:
  wait for the next scheduled wakeup or an incoming DaemonRequest
  drain any requests that arrived; apply + persist them; reply to each
    (only after the persist succeeds)
  working = state.lock().clone()
  run_phases(&mut working, now_ms):
    lifecycle::tick, screenshot::plan                              // phase 1, 2a
    screenshot::capture_and_process                                // phase 2b
    screenshot::commit, capture_availability::tick,
      heartbeat::tick, upload::plan_hash_retries                   // phase 2c, 3, 4a
    upload::execute_hash_retries (network)                         // phase 5a
    upload::commit_hash_retries, upload::plan_batch                // phase 5c, 4b
    upload::execute_batch (network)                                // phase 5b
    upload::commit_batch                                           // phase 5d
  compute_next_wakeup, persist, state.lock() = working              // phase 6, 7
```

Hash results are committed _before_ the batch is planned so an event hashed
this tick is eligible for the same tick's batch upload — SPEC.md's phase
list is illustrative, not a literal contract on lock-acquisition count.

Each module is a plain `struct FooState` (`#[derive(Serialize, Deserialize,
Default, Clone)]`, `#[serde(default)]`) plus free functions operating on it —
no trait, no `Observer`, no event dispatch. A module that needs to enqueue
work calls `upload::enqueue(&mut upload_state, now_ms, risk, kind)` directly
instead of publishing an event.

```rust
let daemon = Daemon::new(config, platform, api, state_path)?;
daemon.run_forever(); // blocking; call from its own thread
```

### DaemonState

One flat struct, persisted to `event_state.json` with **the same top-level
field names as the pre-rewrite per-observer keys** (`auth`, `lifecycle`,
`screenshot`, `upload`, `capture_availability`, `heartbeat`) so existing
installs load cleanly and `client/*/src/config.rs::read_auth_state` (which
reads the `auth` key directly, bypassing the daemon) needs no changes:

```rust
pub struct DaemonState {
    pub version: u32,
    pub auth: AuthState,
    pub lifecycle: LifecycleState,
    pub screenshot: ScreenshotState,
    pub upload: UploadState,
    pub capture_availability: CaptureAvailabilityState,
    pub heartbeat: HeartbeatState,
    pub next_wakeup_at_ms: i64,
    pub last_tick_at_ms: Option<i64>,
}
```

`version` (`DAEMON_STATE_VERSION`) exists so a future breaking change to this
shape has somewhere to branch migration logic, rather than relying solely on
`#[serde(default)]`.

### Responsiveness: a request channel, not a shared mutex

`login`/`logout`/`note_user_stop`/`queue_upload`/`flush_batch_now` build a
`DaemonRequest` (each variant carries its own reply `Sender`), send it on the
`mpsc` channel, and block on the reply — sending is itself the wakeup, since
`run_forever` is blocked in `recv_timeout` waiting for exactly this. The loop
thread is the only code that ever mutates `DaemonState`; a private `apply_*`
function per request type does the actual mutation against the tick's owned
clone (`apply_login`, `apply_logout`, …), while the public method is a thin
wrapper around the channel round trip. Two internal call sites — the
`should_logout` handling at the end of a tick, and the shutdown-time forced
flush — call the `apply_*` function directly rather than the public method,
since calling the public (channel-based) method from the loop thread itself
would deadlock forever waiting on its own reply.

`status()` is the one exception: a direct lock-based read of the shared
snapshot, never routed through the channel — there's nothing to synchronize,
and a channel round trip would needlessly queue a pure read behind an
in-flight tick's network I/O. `request_stop()` stays fire-and-forget, sending
a `DaemonRequest::Stop` the loop drains like any other request.

A `#[cfg(any(test, feature = "testing"))]` bypass (`test_login`, `test_logout`,
…, plus `tick_once_for_test`) calls the same `apply_*` functions directly
under a lock instead of going through the channel, so the single-threaded
`Scenario` test harness (which never runs `run_forever` on a background
thread) can drive the daemon synchronously.

### The 6 modules

| Module                 | Owns                                                                        | Notable functions                                                                                                                      |
| ---------------------- | --------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `auth`                 | nothing (writes into `AuthState`/`ScreenshotState`/`UploadState` passed in) | `login()`, `logout()`                                                                                                                  |
| `lifecycle`            | `LifecycleState` (`late_wakeups: VecDeque<i64>`)                            | `tick()`, `note_user_stop()`                                                                                                           |
| `screenshot`           | `ScreenshotState`                                                           | `plan()`, `capture_and_process()`, `commit()`                                                                                          |
| `upload`               | `UploadState`                                                               | `enqueue()`, `plan_hash_retries()`/`execute_hash_retries()`/`commit_hash_retries()`, `plan_batch()`/`execute_batch()`/`commit_batch()` |
| `capture_availability` | `CaptureAvailabilityState`                                                  | `note_failure()`, `tick()`                                                                                                             |
| `heartbeat`            | `HeartbeatState`                                                            | `tick()` (reads `upload.device_credentials` for auth — no separate flag)                                                               |
| `status`               | nothing                                                                     | `build()` — pure `ServiceStatus` assembly from `&AuthState`/`&UploadState`                                                             |

### Screenshot dedup (two gates) + random cadence

`screenshot::plan` draws the next capture time via an **exponential
inter-arrival** (`next = now + (-mean_ms * ln(1 - u))`, `u` from
`RandomSource`) rather than a fixed interval — SPEC.md §3's "every second has
the same chance" requirement. Two gates then run:

1. **Lock / screensaver gate** — checked in `plan` _before_ capturing via
   `is_locked_or_screensaver()`. While locked/screensaving the user cannot be
   viewing real content, so the capture is skipped entirely; the next time is
   still drawn so pacing continues. Fails safe to `false` (fall back to the
   diff gate) when the state is unknown.
2. **Screen-change diff gate** — in `capture_and_process`, the frame is
   reduced to a grayscale grid fingerprint (`module/screenshot/fingerprint.rs`)
   and compared against the **last uploaded** fingerprint (never the previous
   capture, so slow sub-threshold drift eventually crosses the threshold).

### State persistence

`Daemon::new` loads `DaemonState` from `event_state.json`
(`state::load_state`, generic over any `Default + DeserializeOwned` type
now, not just `serde_json::Value`); every tick's final locked phase persists
it back via `state::store_state` (atomic tmp+rename).

### IPC: one file, one channel type (Linux/Mac only)

Windows/Android/iOS need no code here at all: each holds one process-global
`Arc<Daemon<..>>` and calls its methods directly, which already **is** the
one channel type in this system (`DaemonRequest`, see above). Linux/Mac
additionally run their CLI/tray as a separate OS process, so `ipc.rs` adds a
thin cross-process translator on top of those same `Daemon` methods:

- **`spawn_server`** binds a Unix socket and spawns one thread total —
  `loop { accept (blocking); serve that connection to completion; accept
again }` — decoding a newline-JSON `WireRequest` off the socket, calling
  the matching `Daemon` method (which internally round-trips the
  `DaemonRequest` channel exactly like an in-process caller would), and
  encoding the `WireReply` back. Only one client is ever connected at a
  time — a second `connect()` simply blocks in the OS listen backlog until
  the first disconnects — so there's no concurrent-connection bookkeeping.
- The protocol is strictly request/reply — **the daemon never writes
  unprompted**. The CLI/tray connects, sends one request, reads the reply,
  and disconnects, so a push had nowhere to land anyway; both platforms
  learn about a logout (explicit, an implicit revoke during `login()`, or a
  server-forced one on 401/404) from their next `get_status()` poll. Keeping
  it one-directional is also what lets `Daemon` stay free of any `ipc`
  dependency, and therefore of any `target_os` cfg.
- **`ClientController`** is the client side: connect, then block on each
  request/reply round trip.
- `ipc.rs` gates itself with an inner `#![cfg(any(target_os = "linux",
  target_os = "macos"))]` rather than being gated at its `mod` declaration,
  which confines `target_os` in this crate to that single line. `lib.rs`
  declares the module unconditionally and deliberately does **not** re-export
  its types; platform crates name `virtue_core::ipc::…` directly.

## Platform process model

Every platform now drives the **same** `Daemon::run_forever()` loop; only how
each host language reaches it differs.

### Linux / Mac — separate daemon process

The daemon runs as a separate process. `run_forever()` runs on its own
thread; `ipc::spawn_server` spawns the IPC-serving thread once at startup and
returns immediately — no polling from the platform's main loop. Mac's main
thread instead polls a local boot-vs-monotonic divergence check
(`MacPlatformHooks::boot_clock_ms`/`monotonic_clock_ms` — inherent methods,
not part of `LifecycleHooks`) purely for a post-wake UX nudge
(`daemon.flush_batch_now()`); this is independent daemon-loop plumbing, not
part of the core alerting model.

### Windows — in-process `Arc<Daemon>`

No separate process. `resident_monitor::start_monitoring()` builds a
`Daemon` once, spawns `run_forever()` on a background thread, and holds
`Arc<Daemon<...>>` in process-global state. `app_login`/`app_logout`/
`status_snapshot`/the stop functions call the daemon's synchronous methods
directly — the daemon's own mutex+condvar already provides the
responsiveness the old hand-rolled `MonitorCommand` queue existed for.

### Android — JNI entry points, one shared `Arc<Daemon>`

`nativeInit` builds a `Daemon` once and stores it in process-global state
(`AndroidCore.daemon: Arc<Daemon<...>>`). Every other `native*` call is a
direct method call on that shared daemon (`nativeLogin` → `daemon.login()`,
`nativeRunDaemonLoop` → `daemon.run_forever()`, `nativeStopDaemon` →
`daemon.request_stop()`). `nativeIsLoggedIn`/`nativeGetDeviceId` still read
`event_state.json`'s `auth` key directly from disk rather than going through
the daemon, unchanged from before.

### iOS — same C-FFI/`Arc<Daemon>` shape as Android, lifecycle disabled

`virtue_ios_native_init` builds one `Daemon<IosPlatformHooks, HttpApiClient>`
and stores it in process-global state, same pattern as Android's JNI bridge;
every other `virtue_ios_native_*` call is a direct method call on that shared
daemon. `client/ios/rust` is its own standalone Cargo workspace (not a
`client/Cargo.toml` member — this predates the daemon rewrite), so it isn't
covered by `cargo check --workspace` from `client/`; it has its own CI job
(`.github/workflows/client-ios.yml`) that builds it directly.

`IosPlatformHooks::lifecycle_enabled()` returns `false`: the monitoring
process is a short-lived Safari extension host that the OS can suspend the
instant the device locks, with no notification delivered to that process and
no boot/shutdown/session API surface available to it at all — every stall
looks identical to every other, so there's no way to build a meaningful "late
wakeup" signal there. `run_phases` skips `lifecycle::tick` entirely
when this returns `false` (see "PlatformHooks" below) — screenshot capture,
upload, and everything else still runs normally; only the tamper-detection
check is disabled.

## Config model

`Config` fields:

- `api_base_url` — REST API base URL
- `device_name` — stable device identifier
- `platform_name` — e.g. `"linux"`, `"mac"`, `"windows"`
- `state_dir` — directory for all state files
- `screenshot_interval` — mean of the random exponential draw (SPEC.md §3), default 60s in tests
- `batch_interval` — default 60 s

There is no runtime override mechanism: `api_base_url`, `capture_interval_seconds`,
and `batch_window_seconds` are baked in at **compile time** via `env!()`, exactly
like `DEFAULT_API_BASE_URL`. `client/core/build.rs` reads
`VIRTUE_DEFAULT_API_URL`, `VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS`, and
`VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS` from the process environment — falling
back to an optional `client/.env` file (gitignored; see `client/.env.example`)
for any of those not already set by a real env var — and emits them via
`cargo:rustc-env`. `config.rs` exposes them as `DEFAULT_API_BASE_URL` (const)
and `default_capture_interval_seconds()`/`default_batch_window_seconds()`
(functions, since integer parsing needs a body). Real process/CI env vars
always take precedence over `.env`. The interval floors (15s capture, 1s
batch) are enforced as a `panic!` in `build.rs`, not a runtime clamp — an
invalid or too-low value fails the build instead of silently getting clamped.
Because every platform crate depends on `client/core`, and Cargo always runs a
dependency's own `build.rs` when compiling it, this one `client/.env` file
covers every platform.

## State files (under `Config.state_dir`)

| File               | Owner    | Purpose                                                                                          |
| ------------------ | -------- | ------------------------------------------------------------------------------------------------ |
| `event_state.json` | `Daemon` | Serialised `DaemonState` (screenshot schedule, upload queues, lifecycle late-wakeup array, auth) |

Permanent hash-upload failures (400 responses) go through a normal
`tracing::error!` call in `upload::commit_hash_retries`, not a dedicated file
— every platform already has durable `tracing`-based logging (Linux ->
stdout/journald; Mac/Windows/Android/iOS -> daily rotating file, see
`src/logging.rs`).

## PlatformHooks

Keep the traits minimal. Platforms implement `ScreenshotHooks` and `LifecycleHooks`:

```rust
// ScreenshotHooks
fn take_screenshot(&self) -> CoreResult<Screenshot>;
fn get_time_utc_ms(&self) -> CoreResult<i64>;             // default: SystemTime::now()
fn is_locked_or_screensaver(&self) -> CoreResult<bool>;   // default: Ok(false)

// LifecycleHooks
fn get_utc_clock_ms(&self) -> CoreResult<i64>;             // default: SystemTime::now()
fn get_monotonic_clock_ms(&self) -> CoreResult<i64>;       // default: falls back to get_utc_clock_ms
fn get_last_login_utc_ms(&self) -> CoreResult<Option<i64>>;
fn get_last_logout_utc_ms(&self) -> CoreResult<Option<i64>>;
fn lifecycle_enabled(&self) -> bool;                        // default: true
```

`lifecycle_enabled` lets a platform opt out of the late-wakeup tamper check
(`run_phases` skips `lifecycle::tick` entirely when it returns
`false`) while keeping everything else — screenshot capture, upload,
status — running normally. iOS is the only platform that overrides it to
`false`, for the reason described above.

`get_monotonic_clock_ms` (a clock that doesn't advance while the system is
suspended) feeds only `lifecycle::tick`'s suspend evidence (`../SPEC.md`
§2) — a third excuse, alongside login/logout evidence, that can only ever
*add* an excuse (its default falls back to `get_utc_clock_ms`, under which
it never triggers). Screenshot scheduling itself is unaffected and still
paces off the wall clock; this hook isn't on `ScreenshotHooks`. Mac
separately reads its own boot/monotonic clocks for a local post-wake UX
check, but as **inherent methods** on `MacPlatformHooks` (`capture.rs`),
called directly by `mac/src/daemon.rs` — not through this trait, and
unrelated to the suspend evidence above.

`PlatformHooks: ScreenshotHooks + LifecycleHooks` is a blanket impl (`impl<T:
ScreenshotHooks + LifecycleHooks> PlatformHooks for T {}`) — platforms never
implement it directly, and doing so is a compile error (conflicting impls).

`get_last_login_utc_ms`/`get_last_logout_utc_ms` can be expensive (D-Bus
round-trips, subprocess shell-outs); `lifecycle::tick` calls them directly
every tick with no throttling, which is fine now that the loop itself only
wakes roughly every `screenshot_interval` (minutes), not every second.

## Batch blob format

See `../CLAUDE.md` (repo root) for the exact wire format. Summary:

```
events → encode_batch_event() per event → BatchBuilder::build_upload()
       → msgpack({events: [...]}) → gzip → AES-256-GCM
wire:  nonce[12 bytes] || ciphertext+tag
```

Each upload also wraps the batch key per recipient using HPKE
(`DhkemX25519HkdfSha256 / HkdfSha256 / Aes256Gcm`). The recipient set comes from
the device's `wrapping_keys`, sourced from `upload.settings` — refreshed
opportunistically from the `settings` field embedded in every `GET /d/device` and
`POST /d/batch` response, not from a dedicated pre-batch fetch. A partner added or
removed is therefore picked up with a one-batch lag (the batch in flight when they're
added still uses the old recipient set; the next one uses the new one), not
immediately. A batch upload that fails with 404/401 means the device is gone and
triggers logout; other failures leave the events queued and retry (with
exponential backoff — see `RetryBackoff` below) on the next tick.

Each `BatchUpload` also carries `total_count`/`high_risk_count`/`medium_risk_count`/
`screenshot_count`, sent to the server as `metadata.event_counts` (`{total, high, medium,
screenshot}`). `high_risk_count`/`medium_risk_count` tally events falling in the high
(`risk >= 0.7`) and medium (`0.4 <= risk < 0.7`) bands, thresholds mirroring
`shared-web/risk.ts`; `screenshot_count` tallies `UploadKind::Screenshot` events. These are
computed client-side from the per-event `risk`/kind before encryption, so the server can
summarize tamper activity in partner digest emails without ever decrypting the batch.

## Notify flow

High-risk events (`risk >= lifecycle::EXTRA_HIGH_RISK`) don't travel through a
separate notify queue. When `execute_hash_retries` successfully hashes an event, it
checks whether that event was high-risk and, if so, attaches a `NotifyPayload
{ ts, type, risk, title?, details? }` to the `PendingBatchEvent.notify` field. The
notify payload then rides with that event into whichever batch carries it:
`plan_batch` collects every `notify` present in the events it's about to send into
`BatchUpload.notifications` and uploads them in the same `POST /d/batch` multipart
request, where the server processes them (best-effort) only after the batch itself
has durably persisted. Enqueuing a heartbeat or extra-high-risk event sets
`UploadState.force_flush` (bypassing the batch-interval wait) and, for heartbeats
specifically, `bypass_lock` too (bypassing the screen-lock gate) — see
`RetryBackoff`/gating below. `POST /d/notify` no longer exists.

## Upload gating and backoff

`UploadState` carries two persisted `RetryBackoff { next_attempt_at_ms,
current_backoff_ms }` values (`hash_backoff`, `batch_backoff` — doubling from
1s up to a 20-minute cap, small jitter) so backoff survives a daemon restart,
unlike the pre-rewrite in-memory version. `plan_hash_retries`/`plan_batch`
each require `screen_active || state.bypass_lock` before attempting network
I/O; `plan_batch` additionally requires `post_login_proof_batches_remaining >
0 || interval elapsed || state.force_flush || queue >= MAX_BATCH_ITEMS`.
`Daemon::flush_batch_now()` (and the daemon's own shutdown-time flush) call
`upload::request_immediate_flush`, which sets both flags **and** resets both
backoffs to "ready now" — bypassing the cooldown for one attempt, matching
the old `FlushBatchNow`/`ProcessStopped` "explicit action, try immediately"
semantics.

## Hash chain

Per-event content hashes are uploaded to `POST /hash` independently of batches:

```
content_hash = sha256(ts_le64 || type_utf8 || sorted(key_utf8 || encoded_value))
new_state    = sha256(current_state[32] || content_hash[32])
```

`POST /hash` itself requires a `HashServerToken`, minted by `POST /d/device`, `GET
/d/device`, and `POST /d/batch` — there is no dedicated `POST /d/token`
endpoint. On the wire the token rides as `settings.hash_token` (part of the embedded
`DeviceSettings`, not a sibling field); `api.rs` pulls it out into the separate
`hash_token` field on `RegisteredDevice`/`DeviceState`/`UploadedBatchResponse`.
`UploadState.hash_token_cache: Option<(String, i64)>` (token, fetched-at UTC-ms,
persisted — no longer `Instant`-based) is refreshed from whichever of those
responses arrived most recently and only triggers a `GET /d/device` call
(inside `execute_hash_retries`, unlocked) when that cache goes stale (55
minutes) without a batch having refreshed it in the meantime. `Daemon::new`
also performs one such refresh at startup if already authenticated (SPEC.md
§4's "refreshed on process startup" requirement).

## Testing

The `testing` feature (auto-enabled under `cfg(test)`) exposes:

- `MockApiClient` — records calls, serves canned responses
- `TestPlatformHooks` / `MockClock` — controllable time, queued screenshots
- `TestRandomSource` — canned/deterministic screenshot-cadence draws
- `Scenario` — wraps a `Daemon<TestPlatformHooks, MockApiClient>` built via
  the same `Daemon::new` production uses; `tick()`/`tick_n()`,
  `at_t()`/`advance()`, `login()`/`logout()`/`status()`/`note_user_stop()`/
  `queue_upload()`/`flush_batch_now()` (backed by `Daemon`'s
  `#[cfg(any(test, feature = "testing"))]` bypass methods, which run the same
  `apply_*` mutation logic real callers' channel round trips eventually
  reach, just without a background loop thread), `state()` (lock+clone
  snapshot), `with_state_mut()` (seed a precondition)
- `fixtures` — minimal valid PNG for unit tests

Integration tests live in `core/tests/scenarios.rs` and use `Scenario`.
Per-module behavioral tests live in each module file under `#[cfg(test)] mod tests`.
