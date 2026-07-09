use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;
use virtue_core::{
    CoreError, EventBus, EventChannel, LifecycleHooks, LoginRequested, LoginResult,
    LogoutRequested, LogoutResult, Ping, PlatformConfig, ProcessStarted, ProcessStopped,
    ProcessStoppedReason, Redacted, StatusRequest, StatusResponse, SystemLogoutObserved,
    UserStopRequested, build_default_modules_reqwest, load_state, store_state,
};

use crate::capture::WindowsPlatformHooks;
use crate::config::{ClientPaths, build_core_config};

const ITER_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorStatusSnapshot {
    pub state: String,
    pub logged_in: bool,
    pub pending_request_count: usize,
    pub last_error: Option<String>,
}

impl Default for MonitorStatusSnapshot {
    fn default() -> Self {
        Self {
            state: "stopped".to_string(),
            logged_in: false,
            pending_request_count: 0,
            last_error: None,
        }
    }
}

struct MonitorWorker {
    shutdown: Arc<AtomicBool>,
    command_tx: Sender<MonitorCommand>,
    handle: thread::JoinHandle<()>,
}

enum MonitorCommand {
    NoteStopRequested {
        source: String,
    },
    ProcessStopped(ProcessStoppedReason),
    SystemLogoutObserved {
        utc_ms: i64,
    },
    AppLogin {
        email: String,
        password: String,
        device_name: String,
        response: mpsc::SyncSender<virtue_core::CoreResult<String>>,
    },
    AppLogout {
        response: mpsc::SyncSender<virtue_core::CoreResult<()>>,
    },
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

    let shutdown = Arc::new(AtomicBool::new(false));
    let thread_shutdown = shutdown.clone();
    let (command_tx, command_rx) = mpsc::channel();
    let handle = thread::spawn(move || run_monitor_loop(thread_shutdown, command_rx));

    state.worker = Some(MonitorWorker {
        shutdown,
        command_tx,
        handle,
    });
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

    worker.shutdown.store(true, Ordering::SeqCst);
    let _ = worker.handle.join();

    update_snapshot(|snapshot| {
        snapshot.state = "stopped".to_string();
        snapshot.last_error = None;
    });
    Ok(())
}

pub fn stop_monitoring_from_tray_exit() -> Result<()> {
    send_command(MonitorCommand::NoteStopRequested {
        source: "tray_stop_monitoring".to_string(),
    })?;
    send_command(MonitorCommand::ProcessStopped(ProcessStoppedReason::User))?;
    stop_monitoring()?;
    Ok(())
}

pub fn stop_monitoring_for_system_shutdown() -> Result<()> {
    send_command(MonitorCommand::SystemLogoutObserved {
        utc_ms: current_utc_ms(),
    })?;
    send_command(MonitorCommand::ProcessStopped(
        ProcessStoppedReason::Shutdown,
    ))?;
    stop_monitoring()?;
    Ok(())
}

pub fn stop_monitoring_for_session_logoff() -> Result<()> {
    send_command(MonitorCommand::SystemLogoutObserved {
        utc_ms: current_utc_ms(),
    })?;
    send_command(MonitorCommand::ProcessStopped(ProcessStoppedReason::Other))?;
    stop_monitoring()?;
    Ok(())
}

fn current_utc_ms() -> i64 {
    WindowsPlatformHooks::new().get_utc_clock_ms().unwrap_or(0)
}

fn send_command(command: MonitorCommand) -> Result<()> {
    let controller = controller();
    let state = controller.state.lock().expect("monitor controller lock");
    if let Some(worker) = state.worker.as_ref() {
        let _ = worker.command_tx.send(command);
    }
    Ok(())
}

pub fn status_snapshot() -> MonitorStatusSnapshot {
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
    let (tx, rx) = mpsc::sync_channel(1);
    {
        let state = controller().state.lock().expect("monitor controller lock");
        match state.worker.as_ref() {
            Some(worker) => {
                let _ = worker.command_tx.send(MonitorCommand::AppLogin {
                    email: email.to_string(),
                    password: password.to_string(),
                    device_name: device_name.to_string(),
                    response: tx,
                });
            }
            None => return Err(anyhow::anyhow!("monitoring is not running")),
        }
    }
    rx.recv()
        .map_err(|_| anyhow::anyhow!("monitoring thread disconnected before login completed"))?
        .map_err(anyhow::Error::from)
}

pub fn app_logout() -> Result<()> {
    let (tx, rx) = mpsc::sync_channel(1);
    {
        let state = controller().state.lock().expect("monitor controller lock");
        match state.worker.as_ref() {
            Some(worker) => {
                let _ = worker
                    .command_tx
                    .send(MonitorCommand::AppLogout { response: tx });
            }
            None => return Err(anyhow::anyhow!("monitoring is not running")),
        }
    }
    rx.recv()
        .map_err(|_| anyhow::anyhow!("monitoring thread disconnected before logout completed"))?
        .map_err(anyhow::Error::from)
}

fn run_monitor_loop(shutdown: Arc<AtomicBool>, command_rx: Receiver<MonitorCommand>) {
    let paths = match ClientPaths::discover() {
        Ok(paths) => paths,
        Err(err) => {
            update_snapshot(|s| {
                s.state = "error".to_string();
                s.last_error = Some(err.to_string());
            });
            return;
        }
    };

    if let Err(err) = paths.ensure_dirs() {
        update_snapshot(|s| {
            s.state = "error".to_string();
            s.last_error = Some(err.to_string());
        });
        return;
    }

    let state_path = paths.state_dir.join("event_state.json");
    let config = build_core_config(&paths);

    let modules = match build_default_modules_reqwest(
        config,
        WindowsPlatformHooks::new(),
        PlatformConfig::default(),
    ) {
        Ok(m) => m,
        Err(err) => {
            update_snapshot(|s| {
                s.state = "error".to_string();
                s.last_error = Some(format!("{err:#}"));
            });
            return;
        }
    };

    let bus_state = load_state(&state_path).unwrap_or(serde_json::Value::Null);

    let mut bus = match EventBus::new(modules, bus_state) {
        Ok(b) => b,
        Err(err) => {
            update_snapshot(|s| {
                s.state = "error".to_string();
                s.last_error = Some(format!("{err:#}"));
            });
            return;
        }
    };

    if let Err(err) = (|| -> anyhow::Result<()> {
        bus.send(ProcessStarted)?;
        store_state(&state_path, &bus.iter()?)?;
        Ok(())
    })() {
        update_snapshot(|s| {
            s.state = "error".to_string();
            s.last_error = Some(format!("{err:#}"));
        });
        return;
    }

    loop {
        drain_commands(&mut bus, &state_path, &command_rx);
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        let tick_result = (|| -> anyhow::Result<()> {
            bus.send(Ping)?;
            let state = bus.iter()?;
            store_state(&state_path, &state)?;
            Ok(())
        })();

        match tick_result {
            Ok(()) => match bus.request::<StatusRequest, StatusResponse>(StatusRequest) {
                Ok(resp) => {
                    update_snapshot(|s| {
                        s.logged_in = resp.status.is_authenticated;
                        s.pending_request_count = resp.status.pending_request_count;
                        s.state = if resp.status.is_authenticated {
                            "running".into()
                        } else {
                            "signed_out".into()
                        };
                        s.last_error = None;
                    });
                }
                Err(err) => {
                    update_snapshot(|s| {
                        s.state = "error".into();
                        s.last_error = Some(err.to_string());
                    });
                }
            },
            Err(err) => {
                update_snapshot(|s| {
                    s.state = "error".into();
                    s.last_error = Some(err.to_string());
                });
            }
        }

        wait_for_commands(&mut bus, &state_path, &command_rx, &shutdown, ITER_INTERVAL);
    }

    drain_commands(&mut bus, &state_path, &command_rx);
    let _ = bus.send(Ping);
    if let Ok(state) = bus.iter() {
        let _ = store_state(&state_path, &state);
    }
}

fn handle_command(bus: &mut EventBus, state_path: &Path, command: MonitorCommand) -> Result<()> {
    match command {
        MonitorCommand::NoteStopRequested { source } => {
            bus.send(UserStopRequested { source })?;
            store_state(state_path, &bus.iter()?)?;
        }
        MonitorCommand::ProcessStopped(reason) => {
            bus.send(ProcessStopped(reason))?;
            store_state(state_path, &bus.iter()?)?;
        }
        MonitorCommand::SystemLogoutObserved { utc_ms } => {
            bus.send(SystemLogoutObserved { utc_ms })?;
            store_state(state_path, &bus.iter()?)?;
        }
        MonitorCommand::AppLogin {
            email,
            password,
            device_name,
            response,
        } => {
            let request_result = bus.request::<LoginRequested, LoginResult>(LoginRequested {
                email,
                password: Redacted(password),
                device_name: Some(device_name),
            });
            let _ = store_state(state_path, &bus.iter()?);
            let result = request_result.and_then(|r| {
                if r.success {
                    Ok(r.device_id.unwrap_or_default())
                } else {
                    Err(CoreError::CommandFailed(
                        r.error.unwrap_or_else(|| "login failed".to_string()),
                    ))
                }
            });
            let _ = response.send(result);
        }
        MonitorCommand::AppLogout { response } => {
            let request_result = bus.request::<LogoutRequested, LogoutResult>(LogoutRequested);
            let _ = store_state(state_path, &bus.iter()?);
            let result = request_result.and_then(|r| {
                if r.success {
                    Ok(())
                } else {
                    Err(CoreError::CommandFailed(
                        r.error.unwrap_or_else(|| "logout failed".to_string()),
                    ))
                }
            });
            let _ = response.send(result);
        }
    }
    Ok(())
}

fn drain_commands(bus: &mut EventBus, state_path: &Path, command_rx: &Receiver<MonitorCommand>) {
    while let Ok(command) = command_rx.try_recv() {
        if let Err(e) = handle_command(bus, state_path, command) {
            eprintln!("resident_monitor: command error: {e}");
        }
    }
}

fn wait_for_commands(
    bus: &mut EventBus,
    state_path: &Path,
    command_rx: &Receiver<MonitorCommand>,
    shutdown: &Arc<AtomicBool>,
    duration: Duration,
) {
    let mut remaining = duration;
    while remaining > Duration::ZERO {
        if shutdown.load(Ordering::SeqCst) {
            drain_commands(bus, state_path, command_rx);
            return;
        }

        let tick = remaining.min(Duration::from_secs(1));
        match command_rx.recv_timeout(tick) {
            Ok(command) => {
                if let Err(e) = handle_command(bus, state_path, command) {
                    eprintln!("resident_monitor: command error: {e}");
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                remaining = remaining.saturating_sub(tick);
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn update_snapshot(update: impl FnOnce(&mut MonitorStatusSnapshot)) {
    let controller = controller();
    let mut state = controller.state.lock().expect("monitor controller lock");
    update(&mut state.snapshot);
}
