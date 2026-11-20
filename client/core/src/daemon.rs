use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::api::ApiTransport;
use crate::clock::{self, DaemonClock};
use crate::config::Config;
use crate::error::{CoreError, CoreResult};
use crate::logging::log_warning;
use crate::model::{AuthState, ServiceStatus, UploadKind};
use crate::module::auth;
use crate::module::capture_availability::{self, CaptureAvailabilityState};
use crate::module::dns_filter::{self, DnsFilterState, DnsResolver, SystemDnsResolver};
use crate::module::errors::{self, ErrorState};
use crate::module::heartbeat::{self, HeartbeatState};
use crate::module::lifecycle::{self, LifecycleState};
use crate::module::screenshot::{
    self, CaptureOutcome, Scoring, ScreenshotState, risk_classifier::RiskClassifier,
};
use crate::module::status;
use crate::module::upload::{self, Upload, UploadState};
use crate::platform::PlatformHooks;
use crate::rng::{OsRandomSource, RandomSource};
use crate::state::{load_state, lock_state, store_state};
use virtue_text_detection::ScreenshotOCR;

/// Bump whenever `DaemonState`'s shape needs a breaking change that
/// `#[serde(default)]` alone can't absorb, and add the step from the old
/// version to [`DaemonState::upgrade`] (CORE-016). No step changes any data
/// yet — every change so far has been additive/subtractive.
pub const DAEMON_STATE_VERSION: u32 = 1;

/// Loads the state at `path` and runs its upgrade steps (CORE-016). Every
/// read that may be written back goes through this, so each step's changes
/// are what gets saved. Logs nothing, because it runs before every write:
/// `Daemon::new` runs the same steps on its first load and logs them, once
/// per start.
fn load_current_state(path: &Path) -> CoreResult<DaemonState> {
    let mut state: DaemonState = load_state(path)?;
    state.upgrade();
    Ok(state)
}

/// Logs what loading a state saved at `saved_version` does to it.
fn log_state_upgrade(saved_version: u32) {
    if saved_version > DAEMON_STATE_VERSION {
        tracing::warn!(
            version = saved_version,
            current = DAEMON_STATE_VERSION,
            "daemon state was saved by a newer client; not upgrading it"
        );
    } else if saved_version < DAEMON_STATE_VERSION {
        tracing::info!(
            from = saved_version,
            to = DAEMON_STATE_VERSION,
            "daemon state upgraded"
        );
    }
}

/// A caller's request to mutate the daemon's state, serviced only by the
/// loop thread inside `run_forever`. Each variant (other than `Stop`) carries
/// its own reply channel so a caller's public method can block until its
/// request has been applied and persisted, the same way a direct locked call
/// used to behave.
enum DaemonRequest {
    Login {
        email: String,
        password: String,
        device_name: Option<String>,
        reply: mpsc::Sender<CoreResult<String>>,
    },
    Logout {
        reply: mpsc::Sender<CoreResult<()>>,
    },
    NoteUserStop {
        source: String,
        reply: mpsc::Sender<()>,
    },
    NoteUserStart {
        reply: mpsc::Sender<()>,
    },
    QueueUpload {
        upload: Upload,
        reply: mpsc::Sender<()>,
    },
    FlushBatchNow {
        reply: mpsc::Sender<()>,
    },
    ForceCapture {
        reply: mpsc::Sender<()>,
    },
    Stop,
}

/// How long a public method blocks waiting for the loop thread to service
/// its request before giving up with an error.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The daemon's whole persisted state, serialized to `event_state.json`.
/// Top-level field names are kept identical to the pre-rewrite per-observer
/// keys (`auth`, `lifecycle`, `screenshot`, `upload`, `capture_availability`,
/// `heartbeat`) so existing installs load cleanly, and later keys such as
/// `dns_filter` default when absent (CORE-016), and
/// `client/*/src/config.rs::read_auth_state` (which reads the `auth` key
/// directly) needs no changes.
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct DaemonState {
    pub version: u32,
    pub auth: AuthState,
    pub lifecycle: LifecycleState,
    pub screenshot: ScreenshotState,
    pub upload: UploadState,
    pub capture_availability: CaptureAvailabilityState,
    pub heartbeat: HeartbeatState,
    #[serde(default)]
    pub dns_filter: DnsFilterState,
    #[serde(default)]
    pub errors: ErrorState,
    pub next_wakeup_at_ms: i64,
    pub last_tick_at_ms: Option<i64>,
}

impl DaemonState {
    /// An empty state at the current version: version 0 (nothing saved yet)
    /// run through the upgrade steps, the same way a missing file is. A reset
    /// such as a logout starts from this (CORE-016).
    pub(crate) fn initial() -> Self {
        let mut state = Self::default();
        state.upgrade();
        state
    }

    /// Runs each upgrade step from `self.version` up to
    /// [`DAEMON_STATE_VERSION`], in order (CORE-016). Leaves a state saved by
    /// a newer client alone.
    fn upgrade(&mut self) {
        while self.version < DAEMON_STATE_VERSION {
            match self.version {
                // 0 → 1 initializes the state and must not change its data:
                // version 0 means no file, or one saved before versions were
                // recorded, whose data is already in the version 1 format.
                0 => {}
                version => unreachable!("no upgrade step from daemon state version {version}"),
            }
            self.version += 1;
        }
    }
}

/// One sequential daemon loop: check lifecycle, maybe take a screenshot,
/// upload what's queued, pick the next wakeup time, sleep. See
/// `client/core/SPEC.md`.
///
/// `state` is a read-only snapshot outside the loop thread — every mutation
/// goes through `DaemonRequest` and is applied by the loop thread alone,
/// which clones it once per tick, mutates the clone with no locking, then
/// writes the result back (to `state` and to disk) before the next tick.
pub struct Daemon<P: PlatformHooks, A: ApiTransport + Send + Sync + 'static> {
    state: Arc<Mutex<DaemonState>>,
    request_tx: mpsc::Sender<DaemonRequest>,
    request_rx: Mutex<Option<mpsc::Receiver<DaemonRequest>>>,
    platform: P,
    api: A,
    config: Config,
    state_path: PathBuf,
    classifier: Option<Arc<RiskClassifier>>,
    ocr: Option<Arc<ScreenshotOCR>>,
    rng: Arc<dyn RandomSource>,
    dns: Arc<dyn DnsResolver>,
    /// Every time the daemon records, schedules or compares comes from here,
    /// not the system clock. See CORE-027.
    clock: Mutex<DaemonClock>,
}

impl<P: PlatformHooks, A: ApiTransport + Send + Sync + 'static> Daemon<P, A> {
    /// Loads persisted state and, if already authenticated, performs one
    /// `get_device_settings` refresh before returning (SPEC §4's "refreshed
    /// on process startup" requirement).
    pub fn new(config: Config, platform: P, api: A, state_path: PathBuf) -> CoreResult<Self> {
        let mut state: DaemonState = load_state(&state_path)?;
        log_state_upgrade(state.version);
        state.upgrade();

        let (request_tx, request_rx) = mpsc::channel();
        let clock = DaemonClock::start(
            platform.get_time_utc_ms().unwrap_or(0),
            boot_clock_ms(&platform),
            state.lifecycle.last_tick_utc_ms,
        );

        let daemon = Self {
            state: Arc::new(Mutex::new(state)),
            request_tx,
            request_rx: Mutex::new(Some(request_rx)),
            platform,
            api,
            config,
            state_path,
            classifier: screenshot::load_classifier().map(Arc::new),
            ocr: screenshot::load_ocr().map(Arc::new),
            rng: Arc::new(OsRandomSource),
            dns: Arc::new(SystemDnsResolver),
            clock: Mutex::new(clock),
        };

        // First, so the clock is set from the server before anything is
        // recorded, when the server can be reached (CORE-027).
        daemon.refresh_settings_on_startup();

        // Every fresh `Daemon` instance is a new monitoring session: if the
        // previous session ended via `note_user_stop`, resume normally
        // rather than comparing this tick's wakeup against a schedule from
        // before the (now-excused) stop. A no-op when the previous session
        // wasn't stopped (including a plain kill, which must NOT be excused
        // — see CORE-002 and `lifecycle::note_user_start`).
        {
            let now_ms = daemon.now_ms();
            daemon.with_locked_state(|working| {
                daemon.apply_note_user_start(working, now_ms);
                if daemon.platform.lifecycle_enabled() {
                    daemon.apply_note_restart(working, now_ms);
                }
            });
        }

        Ok(daemon)
    }

    /// Swap in a different [`RandomSource`] — used by the test [`Scenario`]
    /// harness to control screenshot-cadence draws deterministically.
    ///
    /// [`Scenario`]: crate::testing::Scenario
    pub fn with_rng(mut self, rng: Arc<dyn RandomSource>) -> Self {
        self.rng = rng;
        self
    }

    /// Swap in a different [`DnsResolver`] for the DNS filter check
    /// (CORE-028) — used by the test [`Scenario`] harness to script answers.
    ///
    /// [`Scenario`]: crate::testing::Scenario
    pub fn with_dns_resolver(mut self, dns: Arc<dyn DnsResolver>) -> Self {
        self.dns = dns;
        self
    }

    /// Test-only: a cloned snapshot of the daemon's current state.
    pub fn state_snapshot(&self) -> DaemonState {
        self.state
            .lock()
            .expect("daemon state lock poisoned")
            .clone()
    }

    /// Test-only: run `f` with exclusive access to the daemon's live state
    /// (e.g. to seed a scenario). Does not persist or nudge the loop.
    pub fn with_state_mut<R>(&self, f: impl FnOnce(&mut DaemonState) -> R) -> R {
        let mut guard = self.state.lock().expect("daemon state lock poisoned");
        f(&mut guard)
    }

    fn refresh_settings_on_startup(&self) {
        let refresh_token = {
            let guard = self.state.lock().expect("daemon state lock poisoned");
            match guard.auth.device_credentials.as_ref() {
                Some(creds) => creds.refresh_token.clone(),
                None => return,
            }
        };
        let result = self.api.get_device_settings(&refresh_token);
        // Nothing is recorded on the clock yet, so nothing moves with it
        // (CORE-027).
        if let Some(server_ms) = self.api.take_server_time_ms() {
            self.lock_clock()
                .sync(server_ms, boot_clock_ms(&self.platform));
        }
        let now_ms = self.now_ms();
        match result {
            Ok(result) => {
                self.with_locked_state(|working| {
                    working.upload.settings = Some(result.settings);
                    working.upload.hash_token_cache = Some((result.hash_token, now_ms));
                });
            }
            Err(err) => {
                log_warning("startup device-settings refresh failed", Some(&err));
                self.with_locked_state(|working| {
                    errors::record(
                        &mut working.errors,
                        now_ms,
                        "settings_refresh",
                        format!("startup device-settings refresh failed: {err}"),
                    );
                });
            }
        }
    }

    fn now_ms(&self) -> i64 {
        self.lock_clock().now_ms(boot_clock_ms(&self.platform))
    }

    fn lock_clock(&self) -> std::sync::MutexGuard<'_, DaemonClock> {
        self.clock.lock().expect("daemon clock lock poisoned")
    }

    /// Sets the clock from the latest server response, if one came since the
    /// last call. A correction moves every persisted time later work is
    /// scheduled or measured from with it, and is reported when it's big.
    /// See CORE-027.
    fn sync_clock(&self, state: &mut DaemonState) {
        let Some(server_ms) = self.api.take_server_time_ms() else {
            return;
        };
        let correction_ms = self
            .lock_clock()
            .sync(server_ms, boot_clock_ms(&self.platform));
        if correction_ms == 0 {
            return;
        }
        tracing::warn!(correction_ms, "daemon clock corrected from the server");
        shift_times(state, correction_ms);
        if correction_ms.abs() > clock::CORRECTION_ALERT_MS && self.platform.lifecycle_enabled() {
            upload::enqueue(
                &mut state.upload,
                self.now_ms(),
                lifecycle::HIGH_RISK_LIFECYCLE_ALERT,
                UploadKind::Alert {
                    message: clock::correction_message(correction_ms),
                },
            );
        }
    }

    /// Writes `state` with the version its upgrade steps reached (CORE-016).
    fn persist(&self, state: &DaemonState) {
        debug_assert!(
            state.version >= DAEMON_STATE_VERSION,
            "persisting daemon state version {} that skipped its upgrade steps",
            state.version
        );
        if let Err(err) = store_state(&self.state_path, state) {
            tracing::error!(error = %err, "daemon: failed to persist state");
        }
    }

    /// Runs one cross-process-safe read-modify-write cycle: acquires the
    /// CORE-016 lock, re-reads `state_path` from disk (rather than trusting
    /// the in-memory cache, which may be stale relative to another process's
    /// writes), lets `f` mutate the freshly-read copy, persists it, and
    /// updates the in-memory cache — all before releasing the lock.
    ///
    /// Every mutate-then-persist path MUST go through this (not call
    /// `persist` directly) so two `Daemon`s against the same `state_path`
    /// (iOS only — see CORE-016) can't race a lost update.
    fn with_locked_state<R>(&self, f: impl FnOnce(&mut DaemonState) -> R) -> R {
        let _lock = match lock_state(&self.state_path) {
            Ok(lock) => Some(lock),
            Err(err) => {
                tracing::error!(
                    error = %err,
                    "daemon: failed to acquire cross-process state lock; proceeding without it"
                );
                None
            }
        };

        let mut working = load_current_state(&self.state_path).unwrap_or_else(|err| {
            tracing::warn!(
                error = %err,
                "daemon: failed to reload state under lock; falling back to in-memory copy"
            );
            self.state
                .lock()
                .expect("daemon state lock poisoned")
                .clone()
        });

        let result = f(&mut working);

        self.persist(&working);
        *self.state.lock().expect("daemon state lock poisoned") = working;

        result
    }

    // ── Apply logic: the only code allowed to mutate a `DaemonState` ───────
    //
    // Each of these mirrors one public request method's effect, but operates
    // on a caller-owned `&mut DaemonState` with no locking, and returns its
    // result instead of replying. Called from `run_forever`'s request-drain
    // step and from the `#[cfg(any(test, feature = "testing"))]` bypass methods below —
    // never from a public method directly, which would deadlock against the
    // loop thread servicing its own request.

    #[allow(clippy::too_many_arguments)]
    fn apply_login(
        &self,
        state: &mut DaemonState,
        email: &str,
        password: &str,
        device_name_override: Option<&str>,
        now_ms: i64,
    ) -> CoreResult<String> {
        auth::login(
            state,
            &self.api,
            &self.config.device_name,
            &self.config.platform_name,
            email,
            password,
            device_name_override,
            now_ms,
        )
    }

    fn apply_logout(&self, state: &mut DaemonState) {
        auth::logout(state, &self.api)
    }

    /// A logout nobody asked for: the server rejected the device. Keeps the
    /// recent errors, which hold the reason (CORE-009).
    fn apply_rejected_device_logout(&self, state: &mut DaemonState) {
        let errors = std::mem::take(&mut state.errors);
        self.apply_logout(state);
        state.errors = errors;
    }

    fn apply_note_user_stop(&self, state: &mut DaemonState, now_ms: i64, source: &str) {
        lifecycle::note_user_stop(&mut state.lifecycle, &mut state.upload, now_ms, source);
    }

    /// Re-enables lifecycle tamper detection after a prior `note_user_stop`.
    /// Resets the wakeup schedule baseline too, but only when a stop was
    /// actually active — see `lifecycle::note_user_start`'s doc comment for
    /// why an unconditional reset would defeat detection of a plain kill.
    fn apply_note_user_start(&self, state: &mut DaemonState, now_ms: i64) {
        if lifecycle::note_user_start(&mut state.lifecycle, &mut state.upload, now_ms) {
            state.next_wakeup_at_ms = 0;
        }
    }

    fn apply_note_restart(&self, state: &mut DaemonState, now_ms: i64) {
        lifecycle::note_restart(&mut state.lifecycle, &mut state.upload, now_ms);
    }

    fn apply_queue_upload(&self, state: &mut DaemonState, now_ms: i64, upload: Upload) {
        upload::enqueue(&mut state.upload, now_ms, upload.risk, upload.kind);
    }

    fn apply_flush_batch_now(&self, state: &mut DaemonState, now_ms: i64) {
        upload::request_immediate_flush(&mut state.upload, now_ms);
    }

    /// Runs a forced (on-demand) capture through the same
    /// `capture_and_process`/`commit` pipeline `run_phases` uses, then
    /// requests an immediate batch flush so the result doesn't wait out the
    /// normal batch interval.
    fn apply_force_capture(&self, state: &mut DaemonState, now_ms: i64) {
        let plan = match screenshot::plan_forced(&state.screenshot, &self.platform) {
            Ok(plan) => plan,
            Err(err) => {
                tracing::error!(phase = "screenshot_plan_forced", error = %err, "daemon phase failed");
                errors::record(
                    &mut state.errors,
                    now_ms,
                    "screenshot_capture",
                    format!("forced capture could not be planned: {err}"),
                );
                None
            }
        };

        let scoring =
            Scoring::for_device(state.upload.settings.as_ref(), &self.config.platform_name);
        let captured = plan.map(|plan| {
            screenshot::capture_and_process(
                plan,
                &self.platform,
                self.classifier.as_deref(),
                scoring,
                self.ocr.as_deref(),
            )
        });
        record_capture_failure(&mut state.errors, captured.as_ref(), now_ms);

        screenshot::commit(
            &mut state.screenshot,
            &mut state.upload,
            &mut state.capture_availability,
            captured,
            now_ms,
        );

        upload::request_immediate_flush(&mut state.upload, now_ms);
    }

    // ── Public request methods ──────────────────────────────────────────
    //
    // Each blocks on a fresh reply channel until the loop thread has applied
    // and persisted the request, matching the old locked-call contract.

    fn call<T>(&self, build: impl FnOnce(mpsc::Sender<T>) -> DaemonRequest) -> CoreResult<T> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.request_tx
            .send(build(reply_tx))
            .map_err(|_| CoreError::InvalidState("daemon loop is not running"))?;
        reply_rx
            .recv_timeout(REQUEST_TIMEOUT)
            .map_err(|_| CoreError::InvalidState("daemon loop unresponsive"))
    }

    pub fn login(
        &self,
        email: &str,
        password: &str,
        device_name: Option<&str>,
    ) -> CoreResult<String> {
        self.call(|reply| DaemonRequest::Login {
            email: email.to_string(),
            password: password.to_string(),
            device_name: device_name.map(str::to_string),
            reply,
        })?
    }

    pub fn logout(&self) -> CoreResult<()> {
        self.call(|reply| DaemonRequest::Logout { reply })?
    }

    /// Pure read — a direct lock on the loop's last-committed snapshot,
    /// never routed through the request channel (nothing to synchronize,
    /// and a channel round-trip would needlessly queue behind an in-flight
    /// tick's network I/O).
    pub fn status(&self) -> ServiceStatus {
        let guard = self.state.lock().expect("daemon state lock poisoned");
        status::build(&guard, &self.config, true)
    }

    pub fn note_user_stop(&self, source: &str) {
        if let Err(err) = self.call(|reply| DaemonRequest::NoteUserStop {
            source: source.to_string(),
            reply,
        }) {
            log_warning("note_user_stop: daemon loop unreachable", Some(&err));
        }
    }

    /// Explicitly marks monitoring as resumed after a prior `note_user_stop`
    /// — see `lifecycle::note_user_start`. `Daemon::new` already calls the
    /// state-only equivalent once at construction, so most platforms never
    /// need this; it exists for platforms like Android where the same
    /// `Daemon` instance's loop can be stopped and resumed without a fresh
    /// `Daemon::new` in between.
    ///
    /// Blocks on the loop's reply, so it must only be called while the loop
    /// is running. A caller that is about to start the loop itself must use
    /// `queue_note_user_start` instead (CORE-022).
    pub fn note_user_start(&self) {
        if let Err(err) = self.call(|reply| DaemonRequest::NoteUserStart { reply }) {
            log_warning("note_user_start: daemon loop unreachable", Some(&err));
        }
    }

    /// `note_user_start` without the wait: enqueues the request and returns
    /// immediately. The loop applies it ahead of anything queued afterward,
    /// including a `login`, as soon as it next runs. For a caller that is
    /// about to run `run_forever` itself, where the blocking form would sit
    /// out its whole timeout waiting on a loop that has not started
    /// (CORE-022).
    pub fn queue_note_user_start(&self) {
        // Nobody waits on this reply; the fire's send just fails quietly.
        let (reply, _) = mpsc::channel();
        let _ = self.request_tx.send(DaemonRequest::NoteUserStart { reply });
    }

    pub fn queue_upload(&self, upload: Upload) {
        if let Err(err) = self.call(|reply| DaemonRequest::QueueUpload { upload, reply }) {
            log_warning("queue_upload: daemon loop unreachable", Some(&err));
        }
    }

    pub fn flush_batch_now(&self) {
        if let Err(err) = self.call(|reply| DaemonRequest::FlushBatchNow { reply }) {
            log_warning("flush_batch_now: daemon loop unreachable", Some(&err));
        }
    }

    /// Forces an immediate screenshot capture (bypassing the normal
    /// interval-due gate, but still honoring the locked/screensaver gate and
    /// the fingerprint dedup gate) and requests an immediate batch flush so
    /// the result uploads without waiting out the normal batch interval.
    pub fn force_capture_now(&self) {
        if let Err(err) = self.call(|reply| DaemonRequest::ForceCapture { reply }) {
            log_warning("force_capture_now: daemon loop unreachable", Some(&err));
        }
    }

    /// Fire-and-forget, same as before — Windows/Android track real shutdown
    /// by joining the loop thread's handle separately.
    pub fn request_stop(&self) {
        let _ = self.request_tx.send(DaemonRequest::Stop);
    }

    // ── Test-only bypass: synchronous, no background thread required ───────
    //
    // `Scenario` drives the daemon single-threaded with no `run_forever`
    // thread, so it can't go through the request channel (nothing would ever
    // service it). These call the same `apply_*` functions the real loop
    // uses, so scenario behavior matches production exactly.

    #[cfg(any(test, feature = "testing"))]
    pub fn test_login(
        &self,
        email: &str,
        password: &str,
        device_name: Option<&str>,
    ) -> CoreResult<String> {
        let now_ms = self.now_ms();
        let mut guard = self.state.lock().expect("daemon state lock poisoned");
        let result = self.apply_login(&mut guard, email, password, device_name, now_ms);
        self.persist(&guard);
        result
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn test_logout(&self) -> CoreResult<()> {
        let mut guard = self.state.lock().expect("daemon state lock poisoned");
        self.apply_logout(&mut guard);
        self.persist(&guard);
        Ok(())
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn test_note_user_stop(&self, source: &str) {
        let now_ms = self.now_ms();
        let mut guard = self.state.lock().expect("daemon state lock poisoned");
        self.apply_note_user_stop(&mut guard, now_ms, source);
        self.persist(&guard);
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn test_note_user_start(&self) {
        let now_ms = self.now_ms();
        let mut guard = self.state.lock().expect("daemon state lock poisoned");
        self.apply_note_user_start(&mut guard, now_ms);
        self.persist(&guard);
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn test_queue_upload(&self, upload: Upload) {
        let now_ms = self.now_ms();
        let mut guard = self.state.lock().expect("daemon state lock poisoned");
        self.apply_queue_upload(&mut guard, now_ms, upload);
        self.persist(&guard);
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn test_flush_batch_now(&self) {
        let now_ms = self.now_ms();
        let mut guard = self.state.lock().expect("daemon state lock poisoned");
        self.apply_flush_batch_now(&mut guard, now_ms);
        self.persist(&guard);
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn test_force_capture(&self) {
        let now_ms = self.now_ms();
        let mut guard = self.state.lock().expect("daemon state lock poisoned");
        self.apply_force_capture(&mut guard, now_ms);
        self.persist(&guard);
    }

    /// Test-only: run one tick directly against the live state, mirroring
    /// the real loop's per-tick body (clone, run phases, write back +
    /// persist) without needing a `run_forever` thread.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn tick_once_for_test(&self) {
        let now_ms = self.now_ms();
        let mut working = self
            .state
            .lock()
            .expect("daemon state lock poisoned")
            .clone();
        let (screen_active, should_logout) = self.run_phases(&mut working, now_ms);
        if should_logout {
            self.apply_rejected_device_logout(&mut working);
        }
        // Read afresh, as `run_tick` does.
        let now_ms = self.now_ms();
        working.next_wakeup_at_ms = self.compute_next_wakeup(&working, now_ms, screen_active);
        working.last_tick_at_ms = Some(now_ms);
        *self.state.lock().expect("daemon state lock poisoned") = working.clone();
        self.persist(&working);
    }

    // ── The loop itself ──────────────────────────────────────────────────

    fn take_request_receiver(&self) -> mpsc::Receiver<DaemonRequest> {
        self.request_rx
            .lock()
            .expect("daemon state lock poisoned")
            .take()
            .expect("run_forever/tick_once called while already running")
    }

    /// Hands the receiver back after a clean exit so a later `run_forever`
    /// or `tick_once` call can take it again — see `run_forever`'s doc
    /// comment for why this needs to be restartable rather than a true
    /// one-shot.
    fn restore_request_receiver(&self, rx: mpsc::Receiver<DaemonRequest>) {
        *self.request_rx.lock().expect("daemon state lock poisoned") = Some(rx);
    }

    /// Applies and persists `requests` (already drained from the channel,
    /// `Stop` filtered out by the caller), replying to each only once that's
    /// durable. A no-op if `requests` is empty. Shared by `run_forever` and
    /// `tick_once` — the only difference between them is how they gather the
    /// batch of requests to hand in here.
    fn apply_requests(&self, requests: Vec<DaemonRequest>, now_ms: i64) {
        if requests.is_empty() {
            return;
        }

        let fires: Vec<Box<dyn FnOnce() + Send>> = self.with_locked_state(|working| {
            let mut fires: Vec<Box<dyn FnOnce() + Send>> = Vec::with_capacity(requests.len());
            for req in requests {
                match req {
                    DaemonRequest::Login {
                        email,
                        password,
                        device_name,
                        reply,
                    } => {
                        let result = self.apply_login(
                            working,
                            &email,
                            &password,
                            device_name.as_deref(),
                            now_ms,
                        );
                        fires.push(Box::new(move || {
                            let _ = reply.send(result);
                        }));
                    }
                    DaemonRequest::Logout { reply } => {
                        self.apply_logout(working);
                        fires.push(Box::new(move || {
                            let _ = reply.send(Ok(()));
                        }));
                    }
                    DaemonRequest::NoteUserStop { source, reply } => {
                        self.apply_note_user_stop(working, now_ms, &source);
                        fires.push(Box::new(move || {
                            let _ = reply.send(());
                        }));
                    }
                    DaemonRequest::NoteUserStart { reply } => {
                        self.apply_note_user_start(working, now_ms);
                        fires.push(Box::new(move || {
                            let _ = reply.send(());
                        }));
                    }
                    DaemonRequest::QueueUpload { upload, reply } => {
                        self.apply_queue_upload(working, now_ms, upload);
                        fires.push(Box::new(move || {
                            let _ = reply.send(());
                        }));
                    }
                    DaemonRequest::FlushBatchNow { reply } => {
                        self.apply_flush_batch_now(working, now_ms);
                        fires.push(Box::new(move || {
                            let _ = reply.send(());
                        }));
                    }
                    DaemonRequest::ForceCapture { reply } => {
                        self.apply_force_capture(working, now_ms);
                        fires.push(Box::new(move || {
                            let _ = reply.send(());
                        }));
                    }
                    DaemonRequest::Stop => unreachable!("filtered out by the caller"),
                }
            }
            fires
        });

        for fire in fires {
            fire();
        }
    }

    /// Runs one normal tick (phases + next-wakeup + persist). Shared by
    /// `run_forever` and `tick_once`.
    fn run_tick(&self, now_ms: i64) {
        self.with_locked_state(|working| {
            let (screen_active, should_logout) = self.run_phases(working, now_ms);
            if should_logout {
                self.apply_rejected_device_logout(working);
            }
            // Read afresh: a tick that uploads can run for a while, and its
            // uploads may have corrected the clock (CORE-027).
            let now_ms = self.now_ms();
            working.next_wakeup_at_ms = self.compute_next_wakeup(working, now_ms, screen_active);
            working.last_tick_at_ms = Some(now_ms);
        });
    }

    /// Best-effort final flush on `Stop`: force any queued hash/batch items
    /// out immediately rather than leaving them to wait out a wakeup that
    /// will never come. Shared by `run_forever` and `tick_once`.
    fn run_stopping_flush(&self, now_ms: i64) {
        self.with_locked_state(|working| {
            self.apply_flush_batch_now(working, now_ms);
            let (_, should_logout) = self.run_phases(working, now_ms);
            if should_logout {
                self.apply_rejected_device_logout(working);
            }
        });
    }

    /// Blocking loop. Each iteration: wait for either the next scheduled
    /// wakeup or an incoming request; apply and persist any requests that
    /// arrived (replying only once that's durable); then, if a request was
    /// applied or the wakeup is actually due, run one tick against a
    /// freshly-reloaded copy of the state and write the result back.
    ///
    /// The wait is capped at 60s because `recv_timeout` measures a clock
    /// that stops while the system is suspended, so an uncapped wait could
    /// overshoot its wall-clock deadline by however long the machine slept.
    /// Waking at the cap without anything due goes straight back to waiting
    /// rather than running a tick (CORE-020). Both the request-application step and the tick go through
    /// `with_locked_state`, so each is its own cross-process-safe
    /// read-modify-write cycle (CORE-016) — there's still no *in-process*
    /// locking in the middle of either one.
    ///
    /// Callable more than once across a `Daemon`'s lifetime — just not
    /// concurrently with itself. Linux/Mac/Windows only ever call this once,
    /// for the life of the process, but Android's accessibility service
    /// starts and stops this same loop repeatedly within one process
    /// (pause/resume monitoring, logout, the service reconnecting), so a
    /// clean return (via `Stop` or channel disconnect) hands the request
    /// receiver back rather than consuming it permanently.
    pub fn run_forever(&self) {
        let rx = self.take_request_receiver();
        loop {
            let next_wakeup_at_ms = self
                .state
                .lock()
                .expect("daemon state lock poisoned")
                .next_wakeup_at_ms;
            let now_ms = self.now_ms();
            let wait_ms = (next_wakeup_at_ms - now_ms).clamp(0, 60_000) as u64;
            let first = match rx.recv_timeout(Duration::from_millis(wait_ms)) {
                Ok(req) => Some(req),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => {
                    self.restore_request_receiver(rx);
                    return;
                }
            };

            let mut requests: Vec<DaemonRequest> = first.into_iter().collect();
            while let Ok(req) = rx.try_recv() {
                requests.push(req);
            }
            let stopping = {
                let had_stop = requests.iter().any(|r| matches!(r, DaemonRequest::Stop));
                requests.retain(|r| !matches!(r, DaemonRequest::Stop));
                had_stop
            };

            let now_ms = self.now_ms();
            let applied_request = !requests.is_empty();
            self.apply_requests(requests, now_ms);

            if stopping {
                self.run_stopping_flush(now_ms);
                self.restore_request_receiver(rx);
                return;
            }

            let due = now_ms
                >= self
                    .state
                    .lock()
                    .expect("daemon state lock poisoned")
                    .next_wakeup_at_ms;
            if applied_request || due {
                self.run_tick(now_ms);
            }
        }
    }

    /// Applies and persists whatever requests are currently queued and,
    /// unless a `Stop` was among them, runs exactly one tick — then returns
    /// without blocking or looping. See CORE-015: for a platform with no
    /// way to keep a background thread alive between invocations (iOS's
    /// Safari-extension native message handler — the OS only guarantees it
    /// runs for the duration of one `beginRequest`/`completeRequest` round
    /// trip, not a persistent background loop; see `architecture.md`), call
    /// this once per invocation instead of `run_forever`.
    ///
    /// Not callable concurrently with `run_forever` or with itself on the
    /// same `Daemon` (same restriction as `run_forever`, enforced the same
    /// way — see `take_request_receiver`).
    pub fn tick_once(&self) {
        self.tick_once_with(false);
    }

    /// `tick_once`, but with a `force_capture_now` applied ahead of the
    /// tick as if it had been queued (CORE-021). A `tick_once` caller has no
    /// loop to service `force_capture_now`'s blocking request, so this is
    /// how it asks for one.
    pub fn tick_once_forced(&self) {
        self.tick_once_with(true);
    }

    fn tick_once_with(&self, force_capture: bool) {
        let rx = self.take_request_receiver();
        let now_ms = self.now_ms();

        let mut requests: Vec<DaemonRequest> = Vec::new();
        while let Ok(req) = rx.try_recv() {
            requests.push(req);
        }
        if force_capture {
            // Nobody waits on this reply; the fire's send just fails quietly.
            let (reply, _) = mpsc::channel();
            requests.push(DaemonRequest::ForceCapture { reply });
        }
        let stopping = {
            let had_stop = requests.iter().any(|r| matches!(r, DaemonRequest::Stop));
            requests.retain(|r| !matches!(r, DaemonRequest::Stop));
            had_stop
        };

        self.apply_requests(requests, now_ms);

        if stopping {
            self.run_stopping_flush(now_ms);
        } else {
            self.run_tick(now_ms);
        }

        self.restore_request_receiver(rx);
    }

    /// One full sequential pass over `working` — lifecycle check, screenshot
    /// plan/capture/commit, capture-availability, heartbeat, DNS filter check, hash retries,
    /// batch upload — with no locking anywhere in here (`working` is an
    /// owned clone, not shared). Returns `(screen_active, should_logout)`;
    /// the caller applies the logout (never this function, to avoid
    /// recursing into the request-channel machinery).
    fn run_phases(&self, working: &mut DaemonState, now_ms: i64) -> (bool, bool) {
        let expected_wakeup = working.next_wakeup_at_ms;
        if self.platform.lifecycle_enabled() {
            let session = lifecycle::SessionTimes::read(&self.platform);
            lifecycle::tick(
                &mut working.lifecycle,
                &mut working.upload,
                &self.platform,
                session,
                now_ms,
                expected_wakeup,
            );
            lifecycle::note_session_events(
                &mut working.lifecycle,
                &mut working.upload,
                session,
                now_ms,
            );
        }

        let screen_active = !self.platform.is_locked_or_screensaver().unwrap_or(false);
        let mean_interval_ms = self.config.screenshot_interval.as_millis() as i64;
        let capture_plan = match screenshot::plan(
            &mut working.screenshot,
            &mut working.upload,
            &self.platform,
            now_ms,
            mean_interval_ms,
            self.rng.as_ref(),
        ) {
            Ok(plan) => plan,
            Err(err) => {
                tracing::error!(phase = "screenshot_plan", error = %err, "daemon phase failed");
                errors::record(
                    &mut working.errors,
                    now_ms,
                    "screenshot_capture",
                    format!("capture could not be planned: {err}"),
                );
                None
            }
        };

        let scoring =
            Scoring::for_device(working.upload.settings.as_ref(), &self.config.platform_name);
        let captured = capture_plan.map(|plan| {
            screenshot::capture_and_process(
                plan,
                &self.platform,
                self.classifier.as_deref(),
                scoring,
                self.ocr.as_deref(),
            )
        });
        record_capture_failure(&mut working.errors, captured.as_ref(), now_ms);

        screenshot::commit(
            &mut working.screenshot,
            &mut working.upload,
            &mut working.capture_availability,
            captured,
            now_ms,
        );
        capture_availability::tick(
            &mut working.capture_availability,
            &mut working.upload,
            now_ms,
        );
        heartbeat::tick(&mut working.heartbeat, &mut working.upload, now_ms);

        let dns_plan = dns_filter::plan(&mut working.dns_filter, &mut working.upload, now_ms);
        if let Some(outcome) = dns_plan.map(|plan| dns_filter::execute(plan, self.dns.as_ref())) {
            dns_filter::commit(
                &mut working.dns_filter,
                &mut working.upload,
                outcome,
                now_ms,
                self.rng.as_ref(),
            );
        }

        // Hash results are committed before the batch is planned so an event
        // hashed this tick is eligible for the same tick's batch upload.
        let hash_plan = upload::plan_hash_retries(&mut working.upload, now_ms, screen_active);
        let hash_outcome = hash_plan.map(|plan| upload::execute_hash_retries(plan, &self.api));
        if let Some(outcome) = hash_outcome {
            errors::record_all(&mut working.errors, now_ms, outcome.errors.clone());
            upload::commit_hash_retries(&mut working.upload, outcome, now_ms);
        }

        let batch_interval_ms = self.config.batch_interval.as_millis() as i64;
        let batch_plan =
            upload::plan_batch(&working.upload, now_ms, batch_interval_ms, screen_active);
        let batch_outcome = batch_plan.map(|plan| upload::execute_batch(plan, &self.api));
        let should_logout = batch_outcome
            .map(|outcome| {
                if let Some((context, message)) = outcome.error() {
                    errors::record(&mut working.errors, now_ms, &context, message);
                }
                upload::commit_batch(&mut working.upload, outcome, now_ms)
            })
            .unwrap_or(false);

        // Last, after the uploads that brought the server's time.
        self.sync_clock(working);

        (screen_active, should_logout)
    }

    /// Picks the next wakeup time (CORE-020): the earliest of the next
    /// scheduled screenshot draw, the next heartbeat, the next DNS filter
    /// check (CORE-028), and the next time
    /// pending hash/batch uploads can make progress — never earlier than
    /// `now_ms`. Uploads held by the screen-lock gate (or missing
    /// credentials/settings) contribute nothing, since no timer clears
    /// those; counting them as due now would spin the loop until they did.
    fn compute_next_wakeup(&self, state: &DaemonState, now_ms: i64, screen_active: bool) -> i64 {
        let mean_interval_ms = self.config.screenshot_interval.as_millis() as i64;
        let batch_interval_ms = self.config.batch_interval.as_millis() as i64;
        let screenshot = state
            .screenshot
            .next_screenshot_at_ms
            .unwrap_or(now_ms + mean_interval_ms);

        [
            heartbeat::next_heartbeat_at_ms(&state.heartbeat, &state.upload),
            dns_filter::next_check_at_ms(&state.dns_filter, &state.upload),
            upload::next_hash_attempt_at_ms(&state.upload, screen_active),
            upload::next_batch_attempt_at_ms(&state.upload, batch_interval_ms, screen_active),
        ]
        .into_iter()
        .flatten()
        .fold(screenshot, i64::min)
        .max(now_ms)
    }
}

/// The platform's boot clock, or the system clock if it can't be read.
fn boot_clock_ms(platform: &impl PlatformHooks) -> i64 {
    platform
        .get_boot_clock_ms()
        .or_else(|_| platform.get_time_utc_ms())
        .unwrap_or(0)
}

/// Moves every persisted time that later work is scheduled or measured from
/// by `delta_ms`, along with a correction of the daemon's clock, so the
/// correction neither holds that work off nor brings it forward. Recorded
/// events, and times kept only to show on the status page, keep their
/// times. 0 means "never" here and stays 0. See CORE-027.
fn shift_times(state: &mut DaemonState, delta_ms: i64) {
    let shift = |ms: &mut i64| {
        if *ms != 0 {
            *ms += delta_ms;
        }
    };
    shift(&mut state.next_wakeup_at_ms);
    state.last_tick_at_ms.as_mut().map(shift);
    state.screenshot.next_screenshot_at_ms.as_mut().map(shift);
    state.upload.last_batch_at_ms.as_mut().map(shift);
    shift(&mut state.upload.hash_backoff.next_attempt_at_ms);
    shift(&mut state.upload.batch_backoff.next_attempt_at_ms);
    if let Some((_, fetched_at_ms)) = state.upload.hash_token_cache.as_mut() {
        shift(fetched_at_ms);
    }
    shift(&mut state.heartbeat.last_heartbeat_ms);
    state
        .capture_availability
        .recent_failures_ms
        .iter_mut()
        .for_each(shift);
    let lifecycle = &mut state.lifecycle;
    lifecycle.last_tick_utc_ms.as_mut().map(shift);
    lifecycle.restart_times.iter_mut().for_each(shift);
    lifecycle.last_restart_alert_at_ms.as_mut().map(shift);
}

/// Mirrors a failed capture into the status page's recent-errors ring
/// (CORE-018). `screenshot::commit` consumes the outcome, so this runs just
/// before it, and is shared by the scheduled and forced capture paths.
fn record_capture_failure(errors: &mut ErrorState, outcome: Option<&CaptureOutcome>, now_ms: i64) {
    if let Some(CaptureOutcome::Failed { reason }) = outcome {
        errors::record(errors, now_ms, "screenshot_capture", reason.clone());
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::testing::Scenario;

    // CORE-014: the loop can be started again after a stop.
    /// Android's accessibility service starts and stops this same loop
    /// repeatedly within one process (pause/resume monitoring, logout, the
    /// service reconnecting) — `run_forever` must tolerate being called
    /// again after a clean stop, not just once per `Daemon`. Regression test
    /// for the receiver being consumed permanently on first use.
    #[test]
    fn run_forever_is_restartable_after_a_clean_stop() {
        let scenario = Scenario::new();
        let daemon = &scenario.daemon;

        std::thread::scope(|scope| {
            for _ in 0..2 {
                let handle = scope.spawn(|| daemon.run_forever());
                // Give the loop a moment to reach its `recv_timeout` wait
                // before stopping it, so this exercises the same
                // request/response path production code relies on rather
                // than racing the thread spawn.
                std::thread::sleep(Duration::from_millis(20));
                daemon.request_stop();
                handle.join().expect("run_forever must not panic");
            }
        });
    }

    /// Android restarts the loop from the thread that is about to run it.
    /// `note_user_start` blocks on a reply that only the loop can send, so it
    /// sat out its whole request timeout there and delayed everything queued
    /// behind it, `login` included (CORE-022). The queued form must return
    /// immediately with no loop running, the loop must apply it first when it
    /// does run, and a `login` queued behind it must be served promptly,
    /// across repeated restarts.
    #[test]
    fn queue_note_user_start_does_not_wait_for_a_loop_and_is_applied_first() {
        let scenario = Scenario::new();
        let daemon = &scenario.daemon;

        std::thread::scope(|scope| {
            for round in 0..2 {
                daemon.test_note_user_stop("test");
                assert!(
                    daemon.state_snapshot().lifecycle.monitoring_stopped,
                    "round {round}: stop must be recorded before the restart"
                );

                let started = Instant::now();
                daemon.queue_note_user_start();
                assert!(
                    started.elapsed() < Duration::from_secs(5),
                    "round {round}: queue_note_user_start must not wait for the loop"
                );
                // Still queued, not yet applied: nothing is servicing the channel.
                assert!(daemon.state_snapshot().lifecycle.monitoring_stopped);

                let handle = scope.spawn(|| daemon.run_forever());

                // The request the platform queues right after starting the
                // loop. Its result doesn't matter here, only that it isn't
                // held up waiting out `REQUEST_TIMEOUT`.
                let started = Instant::now();
                let _ = daemon.login("user@example.com", "password", None);
                assert!(
                    started.elapsed() < Duration::from_secs(5),
                    "round {round}: login was delayed by the loop start"
                );
                assert!(
                    !daemon.state_snapshot().lifecycle.monitoring_stopped,
                    "round {round}: the queued resume must be applied by the loop"
                );

                daemon.request_stop();
                handle.join().expect("run_forever must not panic");
            }
        });
    }
}
