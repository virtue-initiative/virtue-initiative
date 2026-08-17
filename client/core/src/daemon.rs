use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::api::ApiTransport;
use crate::config::Config;
use crate::error::CoreResult;
use crate::logging::log_warning;
use crate::model::{AuthState, ServiceStatus};
use crate::module::auth::{self, Logout};
use crate::module::capture_availability::{self, CaptureAvailabilityState};
use crate::module::heartbeat::{self, HeartbeatState};
use crate::module::lifecycle::{self, LifecycleState};
use crate::module::screenshot::{self, ScreenshotState, risk_classifier::RiskClassifier};
use crate::module::status;
use crate::module::upload::{self, Upload, UploadState};
use crate::platform::PlatformHooks;
use crate::rng::{OsRandomSource, RandomSource};
use crate::state::{load_state, store_state};
use crate::storage::FileStateStore;
use virtue_text_detection::ScreenshotOCR;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::events::RemoteSender;

/// Bump whenever `DaemonState`'s shape needs a breaking change that
/// `#[serde(default)]` alone can't absorb. `Daemon::new` compares the loaded
/// value and logs the transition; there is no real migration to perform for
/// this rewrite (every shape change here is additive/subtractive).
pub const DAEMON_STATE_VERSION: u32 = 1;

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
pub struct Daemon<P: PlatformHooks, A: ApiTransport + Send + Sync + 'static> {
    state: Arc<Mutex<DaemonState>>,
    wake: Arc<Condvar>,
    shutdown: Arc<AtomicBool>,
    platform: P,
    api: A,
    config: Config,
    state_path: PathBuf,
    error_log: FileStateStore,
    classifier: Option<Arc<RiskClassifier>>,
    ocr: Option<Arc<ScreenshotOCR>>,
    rng: Arc<dyn RandomSource>,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    ipc_broadcast: Mutex<Vec<RemoteSender>>,
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
        let error_log = FileStateStore::new(&config.state_dir)?;

        let daemon = Self {
            state: Arc::new(Mutex::new(state)),
            wake: Arc::new(Condvar::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
            platform,
            api,
            config,
            state_path,
            error_log,
            classifier: screenshot::load_classifier().map(Arc::new),
            ocr: screenshot::load_ocr().map(Arc::new),
            rng: Arc::new(OsRandomSource),
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            ipc_broadcast: Mutex::new(Vec::new()),
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

    /// Sets `next_wakeup_at_ms` to `now` and wakes `run_forever`'s sleeping
    /// loop thread (if any) promptly, instead of waiting out the long sleep.
    fn wake_now(&self, guard: &mut MutexGuard<'_, DaemonState>, now_ms: i64) {
        guard.next_wakeup_at_ms = now_ms;
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn broadcast_logout(&self) {
        let mut clients = self
            .ipc_broadcast
            .lock()
            .expect("ipc broadcast lock poisoned");
        clients.retain(|s| s.is_connected() && s.send(Logout).is_ok());
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn broadcast_logout(&self) {}

    /// Registers `sender` to receive daemon-initiated pushes (`Logout`).
    /// Used by `ipc_bridge` when a new connection is accepted.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn add_broadcast_target(&self, sender: RemoteSender) {
        self.ipc_broadcast
            .lock()
            .expect("ipc broadcast lock poisoned")
            .push(sender);
    }

    // ── Synchronous request methods ─────────────────────────────────────

    pub fn login(
        &self,
        email: &str,
        password: &str,
        device_name: Option<&str>,
    ) -> CoreResult<String> {
        let now_ms = self.now_ms();
        let (result, revoked) = {
            let mut guard = self.state.lock().expect("daemon state lock poisoned");
            let DaemonState {
                auth,
                screenshot,
                upload,
                ..
            } = &mut *guard;
            let (result, revoked) = auth::login(
                auth,
                screenshot,
                upload,
                &self.api,
                &self.config.device_name,
                &self.config.platform_name,
                email,
                password,
                device_name,
                now_ms,
            );
            self.wake_now(&mut guard, now_ms);
            self.persist(&guard);
            (result, revoked)
        };
        if revoked {
            self.broadcast_logout();
        }
        self.wake.notify_one();
        result
    }

    pub fn logout(&self) -> CoreResult<()> {
        let now_ms = self.now_ms();
        let revoked = {
            let mut guard = self.state.lock().expect("daemon state lock poisoned");
            let DaemonState {
                auth,
                screenshot,
                upload,
                ..
            } = &mut *guard;
            let revoked = auth::logout(auth, screenshot, upload, &self.api);
            self.wake_now(&mut guard, now_ms);
            self.persist(&guard);
            revoked
        };
        if revoked {
            self.broadcast_logout();
        }
        self.wake.notify_one();
        Ok(())
    }

    /// Pure read — does not nudge the loop.
    pub fn status(&self) -> ServiceStatus {
        let guard = self.state.lock().expect("daemon state lock poisoned");
        status::build(&guard.auth, &guard.upload, guard.last_tick_at_ms)
    }

    pub fn note_user_stop(&self, source: &str) {
        let now_ms = self.now_ms();
        {
            let mut guard = self.state.lock().expect("daemon state lock poisoned");
            lifecycle::note_user_stop(&mut guard.upload, now_ms, source);
            self.wake_now(&mut guard, now_ms);
            self.persist(&guard);
        }
        self.wake.notify_one();
    }

    pub fn queue_upload(&self, upload: Upload) {
        let now_ms = self.now_ms();
        {
            let mut guard = self.state.lock().expect("daemon state lock poisoned");
            upload::enqueue(&mut guard.upload, now_ms, upload.risk, upload.kind);
            self.wake_now(&mut guard, now_ms);
            self.persist(&guard);
        }
        self.wake.notify_one();
    }

    pub fn flush_batch_now(&self) {
        let now_ms = self.now_ms();
        {
            let mut guard = self.state.lock().expect("daemon state lock poisoned");
            upload::request_immediate_flush(&mut guard.upload, now_ms);
            self.wake_now(&mut guard, now_ms);
            self.persist(&guard);
        }
        self.wake.notify_one();
    }

    pub fn request_stop(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.wake.notify_one();
    }

    /// Best-effort final flush, run once by `run_forever` right before it
    /// returns on shutdown — the replacement for the pre-rewrite
    /// `ProcessStopped` event's terminal-flush role. Forces any queued hash/
    /// batch items out immediately (bypassing backoff and the lock gate)
    /// rather than leaving them to wait out a wakeup that will never come.
    fn shutdown_flush(&self) {
        let now_ms = self.now_ms();
        {
            let mut guard = self.state.lock().expect("daemon state lock poisoned");
            upload::request_immediate_flush(&mut guard.upload, now_ms);
        }
        self.tick_once(now_ms);
    }

    // ── The loop itself ──────────────────────────────────────────────────

    /// Blocking loop: `tick_once` then sleep until `next_wakeup_at_ms`, woken
    /// early by any of the synchronous methods above (or shutdown).
    pub fn run_forever(&self) {
        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                self.shutdown_flush();
                return;
            }

            let now_ms = self.now_ms();
            self.tick_once(now_ms);

            let mut guard = self.state.lock().expect("daemon state lock poisoned");
            loop {
                if self.shutdown.load(Ordering::SeqCst) {
                    drop(guard);
                    self.shutdown_flush();
                    return;
                }
                let now_ms = self.now_ms();
                if now_ms >= guard.next_wakeup_at_ms {
                    break;
                }
                let wait_ms = (guard.next_wakeup_at_ms - now_ms).clamp(0, 60_000) as u64;
                guard = self
                    .wake
                    .wait_timeout(guard, Duration::from_millis(wait_ms))
                    .expect("daemon state lock poisoned")
                    .0;
            }
        }
    }

    /// One full sequential pass. Exposed so tests can call it directly
    /// without sleeping.
    pub fn tick_once(&self, now_ms: i64) {
        // Phase 1 + 2a (locked, cheap).
        let (screen_active, capture_plan) = {
            let mut guard = self.state.lock().expect("daemon state lock poisoned");
            let expected_wakeup = guard.next_wakeup_at_ms;
            let DaemonState {
                lifecycle,
                screenshot,
                upload,
                ..
            } = &mut *guard;

            lifecycle::tick(lifecycle, upload, &self.platform, now_ms, expected_wakeup);

            let screen_active = !self.platform.is_locked_or_screensaver().unwrap_or(false);
            let mean_interval_ms = self.config.screenshot_interval.as_millis() as i64;
            let capture_plan = match screenshot::plan(
                screenshot,
                upload,
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

            (screen_active, capture_plan)
        };

        // Phase 2b (slow, unlocked).
        let captured = capture_plan.map(|plan| {
            screenshot::capture_and_process(
                plan,
                &self.platform,
                self.classifier.as_deref(),
                self.ocr.as_deref(),
            )
        });

        // Phase 2c, 3, 4a (locked).
        let batch_interval_ms = self.config.batch_interval.as_millis() as i64;
        let hash_plan = {
            let mut guard = self.state.lock().expect("daemon state lock poisoned");
            let DaemonState {
                screenshot,
                upload,
                capture_availability,
                heartbeat,
                ..
            } = &mut *guard;

            screenshot::commit(screenshot, upload, capture_availability, captured, now_ms);
            capture_availability::tick(capture_availability, upload, now_ms);
            heartbeat::tick(heartbeat, upload, now_ms);

            upload::plan_hash_retries(upload, now_ms, screen_active)
        };

        // Phase 5a (network I/O, unlocked).
        let hash_outcome = hash_plan.map(|plan| upload::execute_hash_retries(plan, &self.api));

        // Phase 5c + 4b (locked): commit hash results *before* planning the
        // batch, so an event hashed this tick is eligible for the same
        // tick's batch upload rather than waiting a full extra wakeup.
        let batch_plan = {
            let mut guard = self.state.lock().expect("daemon state lock poisoned");
            if let Some(outcome) = hash_outcome {
                upload::commit_hash_retries(
                    &mut guard.upload,
                    outcome,
                    now_ms,
                    Some(&self.error_log),
                );
            }
            upload::plan_batch(&guard.upload, now_ms, batch_interval_ms, screen_active)
        };

        // Phase 5b (network I/O, unlocked).
        let batch_outcome = batch_plan.map(|plan| upload::execute_batch(plan, &self.api));

        // Phase 5d, 6, 7 (locked).
        let mut should_logout = false;
        {
            let mut guard = self.state.lock().expect("daemon state lock poisoned");
            if let Some(outcome) = batch_outcome {
                should_logout = upload::commit_batch(&mut guard.upload, outcome, now_ms);
            }
            guard.next_wakeup_at_ms = self.compute_next_wakeup(&guard, now_ms);
            guard.last_tick_at_ms = Some(now_ms);
            self.persist(&guard);
        }

        if should_logout {
            let _ = self.logout();
        }
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
