use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;
use virtue_core::events::{Event, ProcessStoppedReason};
use virtue_core::{MonitorService, UserSessionState};

use crate::capture::WindowsPlatformHooks;
use crate::config::{ClientPaths, build_core_config};

const ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(20);
const LOOP_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorStatusSnapshot {
    pub state: String,
    pub logged_in: bool,
    pub pending_request_count: usize,
    pub last_screenshot_at_ms: Option<i64>,
    pub last_error: Option<String>,
}

impl Default for MonitorStatusSnapshot {
    fn default() -> Self {
        Self {
            state: "stopped".to_string(),
            logged_in: false,
            pending_request_count: 0,
            last_screenshot_at_ms: None,
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
    NoteStopRequested { source: String },
    ProcessStopped(ProcessStoppedReason),
    UserSessionChanged(UserSessionState),
    ComputerSuspended,
    ComputerResumed,
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
    send_command(MonitorCommand::ProcessStopped(
        ProcessStoppedReason::Shutdown,
    ))?;
    stop_monitoring()?;
    Ok(())
}

pub fn stop_monitoring_for_session_logoff() -> Result<()> {
    send_command(MonitorCommand::UserSessionChanged(
        UserSessionState::LoggedOut,
    ))?;
    send_command(MonitorCommand::ProcessStopped(ProcessStoppedReason::Other))?;
    stop_monitoring()?;
    Ok(())
}

pub fn notify_session_logon() -> Result<()> {
    send_command(MonitorCommand::UserSessionChanged(
        UserSessionState::LoggedIn,
    ))
}

pub fn notify_session_logoff() -> Result<()> {
    send_command(MonitorCommand::UserSessionChanged(
        UserSessionState::LoggedOut,
    ))
}

pub fn notify_suspend() -> Result<()> {
    send_command(MonitorCommand::ComputerSuspended)
}

pub fn notify_resume() -> Result<()> {
    send_command(MonitorCommand::ComputerResumed)
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
            snapshot.last_screenshot_at_ms = None;
        } else if snapshot.state == "signed_out" || snapshot.state == "stopped" {
            snapshot.state = "starting".to_string();
        }
    });
}

fn run_monitor_loop(shutdown: Arc<AtomicBool>, command_rx: Receiver<MonitorCommand>) {
    let paths = match ClientPaths::discover() {
        Ok(paths) => paths,
        Err(err) => {
            update_snapshot(|snapshot| {
                snapshot.state = "error".to_string();
                snapshot.last_error = Some(err.to_string());
            });
            return;
        }
    };

    if let Err(err) = paths.ensure_dirs() {
        update_snapshot(|snapshot| {
            snapshot.state = "error".to_string();
            snapshot.last_error = Some(err.to_string());
        });
        return;
    }

    let mut service =
        match MonitorService::setup(build_core_config(&paths), WindowsPlatformHooks::new()) {
            Ok(service) => service,
            Err(err) => {
                update_snapshot(|snapshot| {
                    snapshot.state = "error".to_string();
                    snapshot.last_error = Some(format!("{err:#}"));
                });
                return;
            }
        };

    service.queue_event(Event::ProcessStarted);
    service.queue_event(Event::UserSessionChanged(UserSessionState::LoggedIn));
    let _ = service.run_event_loop_iter();

    loop {
        drain_commands(&mut service, &command_rx);
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        match service.loop_iteration() {
            Ok(_outcome) => {
                if let Ok(status) = service.status() {
                    update_snapshot(|snapshot| {
                        snapshot.logged_in = status.is_authenticated;
                        snapshot.pending_request_count = status.pending_request_count;
                        snapshot.last_error = None;
                        snapshot.state = if status.is_authenticated {
                            "running".to_string()
                        } else {
                            "signed_out".to_string()
                        };
                    });
                } else {
                    update_snapshot(|snapshot| {
                        snapshot.last_error = None;
                        snapshot.state = "running".to_string();
                    });
                }
                wait_for_commands(&mut service, &command_rx, &shutdown, LOOP_INTERVAL);
            }
            Err(err) => {
                let message = err.to_string();
                update_snapshot(|snapshot| {
                    snapshot.state = "error".to_string();
                    snapshot.last_error = Some(message);
                });
                wait_for_commands(&mut service, &command_rx, &shutdown, ERROR_RETRY_INTERVAL);
            }
        }
    }

    drain_commands(&mut service, &command_rx);
    let _ = service.run_event_loop_iter();
    let _ = service.mark_stopped();
}

fn handle_command(service: &mut MonitorService<WindowsPlatformHooks>, command: MonitorCommand) {
    match command {
        MonitorCommand::NoteStopRequested { source } => {
            let _ = service.note_stop_requested_by_user(&source);
        }
        MonitorCommand::ProcessStopped(reason) => {
            service.queue_event(Event::ProcessStopped(reason));
            let _ = service.run_event_loop_iter();
        }
        MonitorCommand::UserSessionChanged(state) => {
            service.queue_event(Event::UserSessionChanged(state));
            let _ = service.run_event_loop_iter();
        }
        MonitorCommand::ComputerSuspended => {
            service.queue_event(Event::ComputerSuspended);
            let _ = service.run_event_loop_iter();
        }
        MonitorCommand::ComputerResumed => {
            service.queue_event(Event::ComputerResumed);
            let _ = service.run_event_loop_iter();
        }
    }
}

fn drain_commands(
    service: &mut MonitorService<WindowsPlatformHooks>,
    command_rx: &Receiver<MonitorCommand>,
) {
    while let Ok(command) = command_rx.try_recv() {
        handle_command(service, command);
    }
}

fn wait_for_commands(
    service: &mut MonitorService<WindowsPlatformHooks>,
    command_rx: &Receiver<MonitorCommand>,
    shutdown: &Arc<AtomicBool>,
    duration: Duration,
) {
    let mut remaining = duration;
    while remaining > Duration::ZERO {
        if shutdown.load(Ordering::SeqCst) {
            drain_commands(service, command_rx);
            return;
        }

        let tick = remaining.min(Duration::from_secs(1));
        match command_rx.recv_timeout(tick) {
            Ok(command) => handle_command(service, command),
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
