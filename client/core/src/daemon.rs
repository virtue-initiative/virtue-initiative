use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::api::ApiTransport;
use crate::config::Config;
use crate::error::{CoreError, CoreResult};
use crate::logging::log_warning;
use crate::model::{AuthState, ServiceStatus};
use crate::module::auth;
use crate::module::capture_availability::{self, CaptureAvailabilityState};
use crate::module::heartbeat::{self, HeartbeatState};
use crate::module::lifecycle::{self, LifecycleState};
use crate::module::screenshot::{self, ScreenshotState, risk_classifier::RiskClassifier};
use crate::module::status;
use crate::module::upload::{self, Upload, UploadState};
use crate::platform::PlatformHooks;
use crate::rng::{OsRandomSource, RandomSource};
use crate::state::{load_state, store_state};
use virtue_text_detection::ScreenshotOCR;

/// Bump whenever `DaemonState`'s shape needs a breaking change that
/// `#[serde(default)]` alone can't absorb; `Daemon::new` logs the version
/// transition. No real migration step exists yet — every change so far has
/// been additive/subtractive.
pub const DAEMON_STATE_VERSION: u32 = 1;

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
    QueueUpload {
        upload: Upload,
        reply: mpsc::Sender<()>,
    },
    FlushBatchNow {
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
/// `heartbeat`) so existing installs load cleanly and
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
    pub next_wakeup_at_ms: i64,
    pub last_tick_at_ms: Option<i64>,
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
}

impl<P: PlatformHooks, A: ApiTransport + Send + Sync + 'static> Daemon<P, A> {
    /// Loads persisted state and, if already authenticated, performs one
    /// `get_device_settings` refresh before returning (SPEC §4's "refreshed
    /// on process startup" requirement).
    pub fn new(config: Config, platform: P, api: A, state_path: PathBuf) -> CoreResult<Self> {
        let mut state: DaemonState = load_state(&state_path)?;
        if state.version != DAEMON_STATE_VERSION {
            tracing::info!(
                from = state.version,
                to = DAEMON_STATE_VERSION,
                "daemon state version upgraded"
            );
            state.version = DAEMON_STATE_VERSION;
        }

        let (request_tx, request_rx) = mpsc::channel();

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
        };

        daemon.refresh_settings_on_startup();
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
        let now_ms = self.now_ms();
        match self.api.get_device_settings(&refresh_token) {
            Ok(result) => {
                let mut guard = self.state.lock().expect("daemon state lock poisoned");
                guard.upload.settings = Some(result.settings);
                guard.upload.hash_token_cache = Some((result.hash_token, now_ms));
                self.persist(&guard);
            }
            Err(err) => {
                log_warning("startup device-settings refresh failed", Some(&err));
            }
        }
    }

    fn now_ms(&self) -> i64 {
        self.platform.get_time_utc_ms().unwrap_or(0)
    }

    fn persist(&self, state: &DaemonState) {
        if let Err(err) = store_state(&self.state_path, state) {
            tracing::error!(error = %err, "daemon: failed to persist state");
        }
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
        let DaemonState {
            auth,
            screenshot,
            upload,
            ..
        } = state;
        auth::login(
            auth,
            screenshot,
            upload,
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
        let DaemonState {
            auth,
            screenshot,
            upload,
            ..
        } = state;
        auth::logout(auth, screenshot, upload, &self.api)
    }

    fn apply_note_user_stop(&self, state: &mut DaemonState, now_ms: i64, source: &str) {
        lifecycle::note_user_stop(&mut state.upload, now_ms, source);
    }

    fn apply_queue_upload(&self, state: &mut DaemonState, now_ms: i64, upload: Upload) {
        upload::enqueue(&mut state.upload, now_ms, upload.risk, upload.kind);
    }

    fn apply_flush_batch_now(&self, state: &mut DaemonState, now_ms: i64) {
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
        status::build(&guard.auth, &guard.upload, guard.last_tick_at_ms, true)
    }

    pub fn note_user_stop(&self, source: &str) {
        if let Err(err) = self.call(|reply| DaemonRequest::NoteUserStop {
            source: source.to_string(),
            reply,
        }) {
            log_warning("note_user_stop: daemon loop unreachable", Some(&err));
        }
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

    /// Test-only: run one tick directly against the live state, mirroring
    /// the real loop's per-tick body (clone, run phases, write back +
    /// persist) without needing a `run_forever` thread.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn tick_once_for_test(&self, now_ms: i64) {
        let mut working = self
            .state
            .lock()
            .expect("daemon state lock poisoned")
            .clone();
        let (_, should_logout) = self.run_phases(&mut working, now_ms);
        if should_logout {
            self.apply_logout(&mut working);
        }
        working.next_wakeup_at_ms = self.compute_next_wakeup(&working, now_ms);
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
            .expect("run_forever called while already running")
    }

    /// Hands the receiver back after a clean exit so a later `run_forever`
    /// call can take it again — see the doc comment on `run_forever` for why
    /// this needs to be restartable rather than a true one-shot.
    fn restore_request_receiver(&self, rx: mpsc::Receiver<DaemonRequest>) {
        *self.request_rx.lock().expect("daemon state lock poisoned") = Some(rx);
    }

    /// Blocking loop. Each iteration: wait for either the next scheduled
    /// wakeup or an incoming request; apply and persist any requests that
    /// arrived (replying only once that's durable); then run one tick
    /// against an owned clone of the state, with no locking in the middle,
    /// and write the result back.
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
            let mut working = self
                .state
                .lock()
                .expect("daemon state lock poisoned")
                .clone();

            if !requests.is_empty() {
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
                                &mut working,
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
                            self.apply_logout(&mut working);
                            fires.push(Box::new(move || {
                                let _ = reply.send(Ok(()));
                            }));
                        }
                        DaemonRequest::NoteUserStop { source, reply } => {
                            self.apply_note_user_stop(&mut working, now_ms, &source);
                            fires.push(Box::new(move || {
                                let _ = reply.send(());
                            }));
                        }
                        DaemonRequest::QueueUpload { upload, reply } => {
                            self.apply_queue_upload(&mut working, now_ms, upload);
                            fires.push(Box::new(move || {
                                let _ = reply.send(());
                            }));
                        }
                        DaemonRequest::FlushBatchNow { reply } => {
                            self.apply_flush_batch_now(&mut working, now_ms);
                            fires.push(Box::new(move || {
                                let _ = reply.send(());
                            }));
                        }
                        DaemonRequest::Stop => unreachable!("filtered out above"),
                    }
                }

                *self.state.lock().expect("daemon state lock poisoned") = working.clone();
                self.persist(&working);
                for fire in fires {
                    fire();
                }
            }

            if stopping {
                // Best-effort final flush: force any queued hash/batch items
                // out immediately rather than leaving them to wait out a
                // wakeup that will never come.
                self.apply_flush_batch_now(&mut working, now_ms);
                let (_, should_logout) = self.run_phases(&mut working, now_ms);
                if should_logout {
                    self.apply_logout(&mut working);
                }
                *self.state.lock().expect("daemon state lock poisoned") = working.clone();
                self.persist(&working);
                self.restore_request_receiver(rx);
                return;
            }

            let (_, should_logout) = self.run_phases(&mut working, now_ms);
            if should_logout {
                self.apply_logout(&mut working);
            }
            working.next_wakeup_at_ms = self.compute_next_wakeup(&working, now_ms);
            working.last_tick_at_ms = Some(now_ms);
            *self.state.lock().expect("daemon state lock poisoned") = working.clone();
            self.persist(&working);
        }
    }

    /// One full sequential pass over `working` — lifecycle check, screenshot
    /// plan/capture/commit, capture-availability, heartbeat, hash retries,
    /// batch upload — with no locking anywhere in here (`working` is an
    /// owned clone, not shared). Returns `(screen_active, should_logout)`;
    /// the caller applies the logout (never this function, to avoid
    /// recursing into the request-channel machinery).
    fn run_phases(&self, working: &mut DaemonState, now_ms: i64) -> (bool, bool) {
        let expected_wakeup = working.next_wakeup_at_ms;
        if self.platform.lifecycle_enabled() {
            lifecycle::tick(
                &mut working.lifecycle,
                &mut working.upload,
                &self.platform,
                now_ms,
                expected_wakeup,
            );
            lifecycle::note_session_events(
                &mut working.lifecycle,
                &mut working.upload,
                &self.platform,
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
                None
            }
        };

        let captured = capture_plan.map(|plan| {
            screenshot::capture_and_process(
                plan,
                &self.platform,
                self.classifier.as_deref(),
                self.ocr.as_deref(),
            )
        });

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

        // Hash results are committed before the batch is planned so an event
        // hashed this tick is eligible for the same tick's batch upload.
        let hash_plan = upload::plan_hash_retries(&mut working.upload, now_ms, screen_active);
        let hash_outcome = hash_plan.map(|plan| upload::execute_hash_retries(plan, &self.api));
        if let Some(outcome) = hash_outcome {
            upload::commit_hash_retries(&mut working.upload, outcome, now_ms);
        }

        let batch_interval_ms = self.config.batch_interval.as_millis() as i64;
        let batch_plan =
            upload::plan_batch(&working.upload, now_ms, batch_interval_ms, screen_active);
        let batch_outcome = batch_plan.map(|plan| upload::execute_batch(plan, &self.api));
        let should_logout = batch_outcome
            .map(|outcome| upload::commit_batch(&mut working.upload, outcome, now_ms))
            .unwrap_or(false);

        (screen_active, should_logout)
    }

    /// Picks the next wakeup time: the earlier of the next scheduled
    /// screenshot draw, the next hash/batch retry attempt (if anything is
    /// pending), or immediately if an urgent flush is outstanding — never
    /// earlier than `now_ms`.
    fn compute_next_wakeup(&self, state: &DaemonState, now_ms: i64) -> i64 {
        let mean_interval_ms = self.config.screenshot_interval.as_millis() as i64;
        let mut candidate = state
            .screenshot
            .next_screenshot_at_ms
            .unwrap_or(now_ms + mean_interval_ms);

        if !state.upload.pending_hash_events.is_empty() {
            candidate = candidate.min(state.upload.hash_backoff.next_attempt_at_ms.max(now_ms));
        }
        if !state.upload.pending_batch_events.is_empty() {
            candidate = candidate.min(state.upload.batch_backoff.next_attempt_at_ms.max(now_ms));
        }
        if state.upload.force_flush {
            candidate = candidate.min(now_ms);
        }

        candidate.max(now_ms)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::testing::Scenario;

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
}
