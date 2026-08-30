use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::thread;

use anyhow::Result;
use serde::Serialize;
use virtue_core::Daemon;
use virtue_core::api::HttpApiClient;
use virtue_core::force_capture::{self, ForcedCaptureOutcome};
use virtue_core::model::ServiceStatus;

use crate::capture::WindowsPlatformHooks;
use crate::config::{ClientPaths, build_core_config};

type WindowsDaemon = Daemon<WindowsPlatformHooks, HttpApiClient>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorStatusSnapshot {
    pub state: String,
    pub logged_in: bool,
    pub pending_request_count: usize,
    pub last_error: Option<String>,
    /// The shared, cross-platform status payload (CORE-010). `None` before
    /// the resident daemon has been built — the Windows-only fields above
    /// still describe the app's own monitor state in that case.
    pub core: Option<ServiceStatus>,
}

impl Default for MonitorStatusSnapshot {
    fn default() -> Self {
        Self {
            state: "stopped".to_string(),
            logged_in: false,
            pending_request_count: 0,
            last_error: None,
            core: None,
        }
    }
}

/// A running daemon plus the OS thread its `run_forever` loop occupies.
/// `daemon` is shared so `app_login`/`app_logout`/`status_snapshot`/the
/// stop functions below can call its synchronous methods directly — the
/// daemon's own mutex+condvar already gives every caller the responsiveness
/// the old hand-rolled `MonitorCommand` queue existed for.
struct MonitorWorker {
    daemon: Arc<WindowsDaemon>,
    handle: thread::JoinHandle<()>,
}

#[derive(Default)]
struct MonitorState {
    worker: Option<MonitorWorker>,
    snapshot: MonitorStatusSnapshot,
}

#[derive(Default)]
struct MonitorController {
    state: Mutex<MonitorState>,
}

static CONTROLLER: OnceLock<MonitorController> = OnceLock::new();

fn controller() -> &'static MonitorController {
    CONTROLLER.get_or_init(MonitorController::default)
}

fn current_daemon() -> Option<Arc<WindowsDaemon>> {
    controller()
        .state
        .lock()
        .expect("monitor controller lock")
        .worker
        .as_ref()
        .map(|w| Arc::clone(&w.daemon))
}

pub fn start_monitoring() -> Result<()> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;
    let controller = controller();

    let mut state = controller.state.lock().expect("monitor controller lock");
    if let Some(worker) = state.worker.as_ref()
        && !worker.handle.is_finished()
    {
        return Ok(());
    }
    if let Some(worker) = state.worker.take() {
        let _ = worker.handle.join();
    }

    state.snapshot.state = if state.snapshot.logged_in {
        "starting".to_string()
    } else {
        "signed_out".to_string()
    };
    state.snapshot.last_error = None;

    let config = build_core_config(&paths);
    let state_path = paths.state_dir.join("event_state.json");
    let daemon = (|| -> Result<Arc<WindowsDaemon>> {
        let api = HttpApiClient::new(&config)?;
        Ok(Arc::new(Daemon::new(
            config,
            WindowsPlatformHooks::new(),
            api,
            state_path,
        )?))
    })();

    let daemon = match daemon {
        Ok(daemon) => daemon,
        Err(err) => {
            state.snapshot.state = "error".to_string();
            state.snapshot.last_error = Some(format!("{err:#}"));
            return Ok(());
        }
    };

    // Reflect the freshly-loaded (and, if authenticated, startup-refreshed)
    // state immediately, rather than waiting for the loop's first tick.
    let status = daemon.status();
    state.snapshot.logged_in = status.is_authenticated;
    state.snapshot.pending_request_count = status.pending_request_count;
    state.snapshot.state = if status.is_authenticated {
        "running".to_string()
    } else {
        "signed_out".to_string()
    };

    let loop_daemon = Arc::clone(&daemon);
    let handle = thread::spawn(move || loop_daemon.run_forever());

    state.worker = Some(MonitorWorker { daemon, handle });
    Ok(())
}

pub fn stop_monitoring() -> Result<()> {
    let controller = controller();
    let worker = {
        let mut state = controller.state.lock().expect("monitor controller lock");
        state.worker.take()
    };

    let Some(worker) = worker else {
        update_snapshot(|snapshot| {
            snapshot.state = "stopped".to_string();
            snapshot.last_error = None;
        });
        return Ok(());
    };

    worker.daemon.request_stop();
    let _ = worker.handle.join();

    update_snapshot(|snapshot| {
        snapshot.state = "stopped".to_string();
        snapshot.last_error = None;
    });
    Ok(())
}

pub fn stop_monitoring_from_tray_exit() -> Result<()> {
    if let Some(daemon) = current_daemon() {
        daemon.note_user_stop("tray_stop_monitoring");
    }
    stop_monitoring()
}

pub fn stop_monitoring_for_os_session_end() -> Result<()> {
    // `Daemon::run_forever` performs its own best-effort final flush when
    // `request_stop` (called by `stop_monitoring` below) makes it return —
    // the replacement for the old explicit `ProcessStopped` signal.
    stop_monitoring()
}

pub fn status_snapshot() -> MonitorStatusSnapshot {
    if let Some(daemon) = current_daemon() {
        let status = daemon.status();
        update_snapshot(|snapshot| {
            snapshot.logged_in = status.is_authenticated;
            snapshot.pending_request_count = status.pending_request_count;
            snapshot.core = Some(status.clone());
            if snapshot.state != "error" {
                snapshot.state = if status.is_authenticated {
                    "running".to_string()
                } else {
                    "signed_out".to_string()
                };
            }
        });
    }
    controller()
        .state
        .lock()
        .expect("monitor controller lock")
        .snapshot
        .clone()
}

pub fn note_login_state(logged_in: bool) {
    update_snapshot(|snapshot| {
        snapshot.logged_in = logged_in;
        if !logged_in {
            snapshot.state = "signed_out".to_string();
            snapshot.last_error = None;
            snapshot.pending_request_count = 0;
        } else if snapshot.state == "signed_out" || snapshot.state == "stopped" {
            snapshot.state = "starting".to_string();
        }
    });
}

pub fn app_login(email: &str, password: &str, device_name: &str) -> Result<String> {
    let daemon = current_daemon().ok_or_else(|| anyhow::anyhow!("monitoring is not running"))?;
    let device_id = daemon
        .login(email, password, Some(device_name))
        .map_err(anyhow::Error::from)?;
    note_login_state(true);
    Ok(device_id)
}

pub fn app_logout() -> Result<()> {
    let daemon = current_daemon().ok_or_else(|| anyhow::anyhow!("monitoring is not running"))?;
    daemon.logout().map_err(anyhow::Error::from)?;
    note_login_state(false);
    Ok(())
}

/// Forces a capture, then waits for the batch it produced to actually reach
/// the server so the UI's confirmation is true rather than optimistic. See
/// `virtue_core::force_capture`.
pub fn force_capture_now() -> Result<ForcedCaptureOutcome> {
    let daemon = current_daemon().ok_or_else(|| anyhow::anyhow!("monitoring is not running"))?;
    let before = daemon.status();
    daemon.force_capture_now();
    let outcome = force_capture::wait_for_upload(
        &before,
        force_capture::DEFAULT_UPLOAD_TIMEOUT,
        force_capture::DEFAULT_POLL_INTERVAL,
        || Ok(daemon.status()),
        std::thread::sleep,
    )?;
    Ok(outcome)
}

fn update_snapshot(update: impl FnOnce(&mut MonitorStatusSnapshot)) {
    let controller = controller();
    let mut state = controller.state.lock().expect("monitor controller lock");
    update(&mut state.snapshot);
}
