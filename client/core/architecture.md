# Core Architecture

Shared Rust library used by all platform clients.

## Design rule

Platform crates provide only raw screen data and OS hooks. `core` owns
everything else: the daemon loop, persistence, retrying, hashing, batch
construction, compression, encryption, and upload semantics.

## Where things are

- `src/daemon.rs`: `Daemon<P, A>` and `DaemonState`, the loop itself.
- `src/module/`: one file per module (see "The modules" below).
- `src/platform.rs`: the `ScreenshotHooks`, `LifecycleHooks` and `PlatformHooks` traits.
- `src/api.rs`: the `ApiTransport` trait and `HttpApiClient`.
- `src/crypto.rs`: AES-256-GCM, HPKE key wrapping and hashing.
- `src/ipc.rs`: the Unix-socket transport for Linux and macOS.
- `src/state.rs`: loading and storing `event_state.json`.
- `src/testing/`: the mocks and the `Scenario` test harness.

## The daemon loop

`core` is structured around **one sequential loop**, not an event bus.
`Daemon<P: PlatformHooks, A: ApiTransport>` owns an `Arc<Mutex<DaemonState>>`
(a read-only snapshot outside the loop thread) and an `mpsc::Sender<DaemonRequest>`.
Each tick runs inside `with_locked_state`: it takes the cross-process file lock,
re-reads `event_state.json` from disk, runs the phases below straight through
against that working copy with no in-process locking in the middle, then persists
the result and writes it back to the shared snapshot before releasing the lock:

```
run_forever loop:
  wait for the next scheduled wakeup (capped at 60s) or an incoming DaemonRequest
  drain any requests that arrived; apply + persist them; reply to each
    (only after the persist succeeds)
  if no request was applied and the wakeup isn't due yet: continue  // CORE-020
  with_locked_state(|working|                                       // CORE-016
    run_phases(working, now_ms):
      lifecycle::tick, screenshot::plan                             // phase 1, 2a
      screenshot::capture_and_process                               // phase 2b
      screenshot::commit, capture_availability::tick,
        heartbeat::tick                                             // phase 2c, 3
      dns_filter::plan, start dns_filter::execute on its own thread,
        dns_filter::commit for checks that finished                 // CORE-029
      upload::plan_hash_retries                                     // phase 4a
      upload::execute_hash_retries (network)                        // phase 5a
      upload::commit_hash_retries, upload::plan_batch               // phase 5c, 4b
      upload::execute_batch (network)                               // phase 5b
      upload::commit_batch                                          // phase 5d
    compute_next_wakeup, persist, state.lock() = working)           // phase 6, 7
```

Hash results are committed _before_ the batch is planned so an event hashed
this tick is eligible for the same tick's batch upload.

The DNS filter check is the one network step that runs off the loop, because a
lookup through the OS resolver has no timeout. `start_dns_check` hands
`dns_filter::execute` to a `DnsCheckRunner` (a thread per check in production,
inline in `Scenario`). The outcome comes back on a channel, tagged with the
check's generation, and a `DaemonRequest::DnsCheckDone` wakes the loop so the
next tick commits it. A check still running after 2 minutes is given up: the
next one may start, and the late outcome's old generation is dropped.

The file lock matters only on iOS, where the app and its Safari extension each
hold a `Daemon` for the same state file. Everywhere else it is uncontended.

Each module is a plain `struct FooState` (`#[derive(Serialize, Deserialize,
Default, Clone)]`, `#[serde(default)]`) plus free functions operating on it.
Modules call each other's functions directly: a module that needs to enqueue
work calls `upload::enqueue(&mut upload_state, now_ms, risk, kind)`.

```rust
let daemon = Daemon::new(config, platform, api, state_path)?;
daemon.run_forever(); // blocking; call from its own thread
```

### DaemonState

One flat struct, persisted to `event_state.json`. Its top-level keys (`auth`,
`lifecycle`, `screenshot`, `upload`, `capture_availability`, `heartbeat`) are
what installs already have on disk, so renaming one breaks every existing
install. Later keys such as `dns_filter` default when absent. Some readers also
bypass the daemon and read the `auth` key directly (`read_auth_state` in
`client/mac/src/config.rs`, and the Android and iOS bridges' signed-in checks):

```rust
pub struct DaemonState {
    pub version: u32,
    pub auth: AuthState,
    pub lifecycle: LifecycleState,
    pub screenshot: ScreenshotState,
    pub upload: UploadState,
    pub capture_availability: CaptureAvailabilityState,
    pub heartbeat: HeartbeatState,
    pub dns_filter: DnsFilterState,
    pub errors: ErrorState,
    pub next_wakeup_at_ms: i64,
    pub last_tick_at_ms: Option<i64>,
}
```

`version` (`DAEMON_STATE_VERSION`) gives a breaking change to this shape an
upgrade step to run, beyond what `#[serde(default)]` can fill in (see "State
persistence" below).

### Responsiveness: a request channel, not a shared mutex

`login`/`logout`/`note_user_stop`/`note_user_start`/`queue_upload`/`flush_batch_now`/`force_capture_now`
build a `DaemonRequest` (each variant carries its own reply `Sender`), send it on the
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
a `DaemonRequest::Stop` the loop drains like any other request. Because that
request stays queued until a loop consumes it, it must not be sent while no
loop is running (CORE-014).

Blocking on the reply means a request can only be served once the loop is
running. A caller that is about to start the loop itself (Android's restart)
therefore uses `queue_note_user_start()`, which only enqueues the resume; the
loop applies it ahead of anything queued after it (CORE-022).

A platform with no thread that can stay alive between calls uses
`tick_once()` instead of `run_forever()`: it applies whatever requests are
queued, runs one tick and returns (CORE-015). iOS's Safari extension calls it
once per message.

A `#[cfg(any(test, feature = "testing"))]` bypass (`test_login`, `test_logout`,
…, plus `tick_once_for_test`) calls the same `apply_*` functions directly
under a lock instead of going through the channel, so the single-threaded
`Scenario` test harness (which never runs `run_forever` on a background
thread) can drive the daemon synchronously.

### The modules

| Module                 | Owns                                                         | Notable functions                                                                                                                                                               |
| ---------------------- | ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `auth`                 | nothing (writes into the `DaemonState` passed in)            | `login()`, `logout()`                                                                                                                                                           |
| `lifecycle`            | `LifecycleState` (`late_wakeups: VecDeque<i64>`)             | `tick()`, `note_user_stop()`                                                                                                                                                    |
| `screenshot`           | `ScreenshotState`                                            | `plan()`, `capture_and_process()`, `commit()`                                                                                                                                   |
| `upload`               | `UploadState`                                                | `enqueue()`, `plan_hash_retries()`/`execute_hash_retries()`/`commit_hash_retries()`, `plan_batch()`/`execute_batch()`/`commit_batch()`                                          |
| `capture_availability` | `CaptureAvailabilityState`                                   | `note_failure()`, `tick()`                                                                                                                                                      |
| `heartbeat`            | `HeartbeatState`                                             | `tick()` (reads `upload.device_credentials` for auth — no separate flag)                                                                                                        |
| `dns_filter`           | `DnsFilterState`                                             | `plan()`/`execute()`/`commit()`, `next_check_at_ms()` — CORE-029; lookups and TLS handshakes go through the injectable `DnsProbe` (`SystemDnsProbe` by default, `Daemon::with_dns_probe`), run off the loop by a `DnsCheckRunner` (`Daemon::with_dns_check_runner`) |
| `errors`               | `ErrorState` (`recent: VecDeque<StatusError>`, capped at 20) | `record()`, `record_all()` — the recent-errors ring the status page shows (CORE-018)                                                                                            |
| `status`               | nothing                                                      | `build()` — pure `ServiceStatus` assembly from `&DaemonState` + `&Config` (CORE-010)                                                                                            |

### Screenshot dedup (two gates) + random cadence

`screenshot::plan` draws the next capture time via an **exponential
inter-arrival** (`next = now + (-mean_ms * ln(1 - u))`, `u` from
`RandomSource`) rather than a fixed interval — CORE-003's "every second has
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
(`state::load_state`, generic over any `Default + DeserializeOwned` type).
Every request batch and every tick then re-reads it under the file lock and
persists it back via `state::store_state` (atomic tmp+rename) before
releasing the lock (`with_locked_state`, CORE-016).

`DaemonState.version` records the file's format. Every load that a write
may follow (`Daemon::new` and each re-read in `with_locked_state`) runs
`DaemonState::upgrade`: one step per
version, in order, from the saved version up to `DAEMON_STATE_VERSION`.
Version 0 means nothing saved yet (a missing or unparseable file, or a
logout's reset via `DaemonState::initial`), and 0 → 1 changes no data,
because clients before CORE-016's versioning rule saved 0 with data already
in the version 1 format. A file from a newer client keeps its version and
runs no steps. A breaking change to the shape bumps `DAEMON_STATE_VERSION`
and adds its step to `upgrade`.

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

Every platform drives the **same** `Daemon`; only how each host language
reaches it differs. All but the iOS Safari extension run `run_forever()`.

### Linux / Mac — separate daemon process

The daemon runs as a separate process. `run_forever()` runs on its own
thread; `ipc::spawn_server` spawns the IPC-serving thread once at startup and
returns immediately — no polling from the platform's main loop. Mac's main
task additionally waits on IOKit wake notifications (`mac/src/power.rs`)
purely for a post-wake UX nudge (`daemon.flush_batch_now()`); this is
independent daemon-loop plumbing, not part of the core alerting model.

### Windows — in-process `Arc<Daemon>`

No separate process. `resident_monitor::start_monitoring()` builds a
`Daemon` once, spawns `run_forever()` on a background thread, and holds
`Arc<Daemon<...>>` in process-global state. `app_login`/`app_logout`/
`status_snapshot`/the stop functions call the daemon's public methods
directly, which send a `DaemonRequest` and block on the reply like any other
caller.

### Android — JNI entry points, one shared `Arc<Daemon>`

`nativeInit` builds a `Daemon` once and stores it in process-global state
(`AndroidCore.daemon: Arc<Daemon<...>>`). Every other `native*` call is a
direct method call on that shared daemon (`nativeLogin` → `daemon.login()`,
`nativeRunDaemonLoop` → `daemon.run_forever()`, `nativeStopDaemon` →
`daemon.request_stop()`). `nativeRunDaemonLoop` and `nativeStopDaemon` share a
`LoopState` (`Idle`/`Running`/`Stopping`): a start waits (up to 15s) for a
previous loop still flushing on its way out instead of failing with `daemon
already running`, and a stop is only sent to a loop that is `Running`, so
Kotlin's unconditional `stopDaemonLoop()` cannot leave a stale `Stop` queued for
the next loop. `nativeIsLoggedIn`/`nativeGetDeviceId` read
`event_state.json`'s `auth` key directly from disk rather than going through
the daemon.

### iOS — two processes, C FFI, lifecycle disabled

`virtue_ios_native_init` builds one `Daemon<IosPlatformHooks, HttpApiClient>`
and stores it in process-global state, same pattern as Android's JNI bridge;
every other `virtue_ios_native_*` call is a direct method call on that shared
daemon.

Two processes do this against the same state file. The Safari extension does
the monitoring: its native message handler calls `virtue_ios_native_tick_once`
once per message, because the OS only keeps that process alive for one
message's round trip. The app runs `run_forever()` only for the length of a
call that blocks on the loop, such as sign-in. The file lock in `with_locked_state`
keeps them from overwriting each other's state (CORE-016).

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
- `screenshot_interval` — mean of the random exponential draw (CORE-003); builds default to 300 s
- `batch_interval` — how long events wait for a batch upload; builds default to 3600 s

The default API URL, capture interval and batch window are baked in at **compile
time** via `env!()`. The one runtime choice is the server a user signs in
to (CORE-028). `login` resolves it through `ApiTransport::resolve_server` and
stores it as `DeviceCredentials.server`, so it lives and dies with the session:
every `ApiTransport` call takes the session's `api_base_url` (`None` for the
default), and `logout`'s state reset returns the device to the default. `client/core/build.rs` reads
`VIRTUE_DEFAULT_API_URL`, `VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS`, and
`VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS` from the process environment — falling
back to the repo-root `.env` (gitignored; see `.env.example`) and then to
`~/.config/virtue-dev.env` (see `docs/development.md`) for any of those not already
set by a real env var — and emits them via `cargo:rustc-env`. `config.rs`
exposes them as `DEFAULT_API_BASE_URL` (const) and
`default_capture_interval_seconds()`/`default_batch_window_seconds()`
(functions, since integer parsing needs a body). Real process/CI env vars
always take precedence, then the repo-root `.env`, then the machine-wide file.
The interval floors (15s capture, 1s batch) are enforced as a `panic!` in
`build.rs`, not a runtime clamp — an invalid or too-low value fails the build
instead of silently getting clamped. Because every platform crate depends on
`client/core`, and Cargo always runs a dependency's own `build.rs` when
compiling it, this one loading path covers every platform.

## State files (under `Config.state_dir`)

| File               | Owner    | Purpose                                                                                          |
| ------------------ | -------- | ------------------------------------------------------------------------------------------------ |
| `event_state.json` | `Daemon` | Serialised `DaemonState` (screenshot schedule, upload queues, lifecycle late-wakeup array, auth) |

Permanent hash-upload failures (400 responses) are logged with
`tracing::error!` in `upload::commit_hash_retries`. Every platform keeps
durable `tracing` logs (Linux -> stdout/journald; Mac/Windows/Android/iOS ->
daily rotating file, see `src/logging.rs`).

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
fn get_boot_clock_ms(&self) -> CoreResult<i64>;            // default: falls back to get_utc_clock_ms
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
suspended) feeds only `lifecycle::tick`'s suspend evidence (CORE-002) — a third excuse, alongside login/logout evidence, that can only ever
_add_ an excuse (its default falls back to `get_utc_clock_ms`, under which
it never triggers). This hook isn't on `ScreenshotHooks`.

The daemon never reads the system clock for a time it records, schedules
or compares. Every such time comes from its own clock (`src/clock.rs`,
CORE-027): the server's time from the latest response's `Date` header
(`ApiTransport::take_server_time_ms`), carried forward on
`get_boot_clock_ms`, a clock that counts from boot including suspend and
that setting the system clock doesn't move. Until the first response it
starts from the system clock, no earlier than the last tick
(`lifecycle.last_tick_utc_ms`). A
correction moves every persisted schedule time with it (`shift_times` in
`daemon.rs`). Mac
separately listens for IOKit wake notifications (`mac/src/power.rs`) for a
local post-wake batch flush — not through this trait, and unrelated to the
suspend evidence above.

`PlatformHooks: ScreenshotHooks + LifecycleHooks` is a blanket impl (`impl<T:
ScreenshotHooks + LifecycleHooks> PlatformHooks for T {}`) — platforms never
implement it directly, and doing so is a compile error (conflicting impls).

`get_last_login_utc_ms`/`get_last_logout_utc_ms` can be expensive (D-Bus
round-trips, subprocess shell-outs); `run_phases` reads them once per tick
(`lifecycle::SessionTimes::read`) and hands the result to both
`lifecycle::tick` and `note_session_events`, with no further throttling.
That's fine because ticks only run when something is actually due
(CORE-020) — roughly every `screenshot_interval` (minutes), not every second.

## Batch blob format

See `batch-format/SPEC.md` (repo root) for the exact wire format. Summary:

```
events → encode_batch_event() per event → BatchBuilder::build_upload()
       → msgpack([event, ...]) → gzip → AES-256-GCM
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
screenshot}`). `high_risk_count`/`medium_risk_count` tally events whose risk rounds to
70% or more, and to 40% up to 70%, thresholds mirroring `shared-web/risk.ts`; `screenshot_count` tallies `UploadKind::Screenshot` events. These are
computed client-side from the per-event `risk`/kind before encryption, so the server can
summarize tamper activity in partner digest emails without ever decrypting the batch.

## Notify flow

High-risk events (`lifecycle::is_extra_high_risk(risk)`, i.e. a risk that rounds to 90% or more, matching the web app's rating) have no
notify queue of their own. When `execute_hash_retries` successfully hashes an event, it
checks whether that event was high-risk and, if so, attaches a `NotifyPayload
{ ts, type, risk, title?, details? }` to the `PendingBatchEvent.notify` field. The
notify payload then rides with that event into whichever batch carries it:
`plan_batch` collects every `notify` present in the events it's about to send into
`BatchUpload.notifications` and uploads them in the same `POST /d/batch` multipart
request, where the server processes them (best-effort) only after the batch itself
has durably persisted. Enqueuing a heartbeat or extra-high-risk event sets
`UploadState.force_flush` (bypassing the batch-interval wait) and, for heartbeats
specifically, `bypass_lock` too (bypassing the screen-lock gate) — see
`RetryBackoff`/gating below.

## Upload gating and backoff

`UploadState` carries two persisted `RetryBackoff { next_attempt_at_ms,
current_backoff_ms }` values (`hash_backoff`, `batch_backoff` — doubling from
1s up to a 20-minute cap, small jitter) so backoff survives a daemon restart.
`plan_hash_retries`/`plan_batch`
each require `screen_active || state.bypass_lock` before attempting network
I/O; `plan_batch` additionally requires `post_login_proof_batches_remaining >
0 || interval elapsed || state.force_flush || queue >= MAX_BATCH_ITEMS`.
A batch always takes every hashed event (BATCH-006), so `plan_hash_retries`
stops hashing once `MAX_BATCH_ITEMS_PER_UPLOAD` hashed events are waiting,
and resumes after that batch lands.
`Daemon::flush_batch_now()` (and the daemon's own shutdown-time flush) call
`upload::request_immediate_flush`, which sets both flags **and** resets both
backoffs to "ready now", so an explicit action skips the cooldown for one
attempt.

## Hash chain

Per-event content hashes are uploaded to `POST /hash` independently of batches:

```
content_hash = sha256(encoded_event_bytes)
new_state    = sha256(current_state[32] || content_hash[32])
```

`POST /hash` itself requires a `HashServerToken`, minted by `POST /d/device`, `GET
/d/device`, and `POST /d/batch`. On the wire the token rides as `settings.hash_token` (part of the embedded
`DeviceSettings`, not a sibling field); `api.rs` pulls it out into the separate
`hash_token` field on `RegisteredDevice`/`DeviceState`/`UploadedBatchResponse`.
`UploadState.hash_token_cache: Option<(String, i64)>` (the token and the UTC ms
it was fetched, persisted) is refreshed from whichever of those
responses arrived most recently and only triggers a `GET /d/device` call
(inside `execute_hash_retries`, unlocked) when that cache goes stale (110
minutes, against the token's 2 hour lifetime) without a batch having refreshed it in the meantime,
or when the hash server answered 401, which clears the cache (a wall clock set back can make an
expired token look fresh). `Daemon::new`
also performs one such refresh at startup if already authenticated (CORE-005's
"refreshed on process startup" requirement).

Each `PendingBatchEvent` keeps the `seq` it was hashed under, and `plan_batch`
sends the first and last as `start_seq`/`end_seq` (API-038) when every event
has one. The API stores a batch only if those match the hash server's chain,
which is how a lost `POST /d/batch` response is handled without holding
hashing. Say an attempt is stored, the chain reset, and the response lost.
`commit_batch` records the attempt's `end_seq` in
`UploadState.unconfirmed_through_seq`, and hashing carries on into the new
chain. The retry holds the old events plus the new ones, so the API answers
`batch_already_stored` with the chain's `first_seq`, and
`drop_stored_events` drops the front events below it, never past
`unconfirmed_through_seq`. If that would drop nothing, it clears every
waiting event's `seq` so the next attempt goes up unchecked, rather than
being refused forever. `hash_chain_moved` (the device hashed more while an
earlier attempt was still in flight) is retried like any failure.

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
