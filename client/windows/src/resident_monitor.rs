use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use virtue_core::{
    CaptureAvailabilityState, ComputerPowerState, LifecycleConfidence, LifecycleObservation,
    LifecycleOrigin, MonitorService, ServiceRole, UserSessionState,
};

#[cfg(target_os = "windows")]
use windows::Win32::System::SystemInformation::GetTickCount64;

use crate::capture::WindowsPlatformHooks;
use crate::config::{ClientPaths, build_core_config};

const ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(20);
const SERVICE_PING_WAKE_PADDING_MS: i64 = 1_000;

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
    NoteStopRequested {
        source: String,
    },
    ServiceStopObserved {
        raw_reason: String,
        shutdown_in_progress: bool,
        explicit_user_stop: bool,
        detected_by: String,
    },
    UserSessionChanged {
        state: UserSessionState,
        origin: LifecycleOrigin,
        detected_by: String,
    },
    ComputerPowerChanged {
        state: ComputerPowerState,
        origin: LifecycleOrigin,
        detected_by: String,
        confidence: LifecycleConfidence,
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
    send_command(MonitorCommand::ServiceStopObserved {
        raw_reason: "tray_exit_menu".to_string(),
        shutdown_in_progress: false,
        explicit_user_stop: true,
        detected_by: "tray_exit_menu".to_string(),
    })?;
    stop_monitoring()?;
    Ok(())
}

pub fn stop_monitoring_for_system_shutdown() -> Result<()> {
    send_command(MonitorCommand::ComputerPowerChanged {
        state: ComputerPowerState::ShuttingDown,
        origin: LifecycleOrigin::SystemShutdown,
        detected_by: "wm_endsession_shutdown".to_string(),
        confidence: LifecycleConfidence::Confirmed,
    })?;
    send_command(MonitorCommand::ServiceStopObserved {
        raw_reason: "wm_endsession_shutdown".to_string(),
        shutdown_in_progress: true,
        explicit_user_stop: false,
        detected_by: "wm_endsession_shutdown".to_string(),
    })?;
    stop_monitoring()?;
    Ok(())
}

pub fn stop_monitoring_for_session_logoff() -> Result<()> {
    send_command(MonitorCommand::UserSessionChanged {
        state: UserSessionState::LoggedOut,
        origin: LifecycleOrigin::SessionLogout,
        detected_by: "wm_endsession_logoff".to_string(),
    })?;
    send_command(MonitorCommand::ServiceStopObserved {
        raw_reason: "session_logout".to_string(),
        shutdown_in_progress: false,
        explicit_user_stop: false,
        detected_by: "wm_endsession_logoff".to_string(),
    })?;
    stop_monitoring()?;
    Ok(())
}

pub fn notify_session_logon() -> Result<()> {
    send_command(MonitorCommand::UserSessionChanged {
        state: UserSessionState::LoggedIn,
        origin: LifecycleOrigin::Unknown,
        detected_by: "wts_session_logon".to_string(),
    })
}

pub fn notify_session_logoff() -> Result<()> {
    send_command(MonitorCommand::UserSessionChanged {
        state: UserSessionState::LoggedOut,
        origin: LifecycleOrigin::SessionLogout,
        detected_by: "wts_session_logoff".to_string(),
    })
}

pub fn notify_suspend() -> Result<()> {
    send_command(MonitorCommand::ComputerPowerChanged {
        state: ComputerPowerState::Suspended,
        origin: LifecycleOrigin::SystemSuspend,
        detected_by: "wm_powerbroadcast_suspend".to_string(),
        confidence: LifecycleConfidence::Confirmed,
    })
}

pub fn notify_resume() -> Result<()> {
    send_command(MonitorCommand::ComputerPowerChanged {
        state: ComputerPowerState::Running,
        origin: LifecycleOrigin::SystemSuspend,
        detected_by: "wm_powerbroadcast_resume".to_string(),
        confidence: LifecycleConfidence::Confirmed,
    })
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

    if let Some((boot_marker, booted_at_ms)) = current_boot_marker() {
        let _ = service.record_lifecycle_observation(LifecycleObservation::BootObserved {
            boot_marker,
            booted_at_ms: Some(booted_at_ms),
            detected_by: "windows_uptime_probe".to_string(),
        });
    }
    let _ = service.record_lifecycle_observation(LifecycleObservation::ServiceStarted {
        role: ServiceRole::PrimaryService,
        detected_by: "windows_resident_app".to_string(),
    });
    let _ = service.record_lifecycle_observation(LifecycleObservation::UserSessionChanged {
        state: UserSessionState::LoggedIn,
        origin: LifecycleOrigin::Unknown,
        detected_by: "windows_app_launch".to_string(),
    });

    loop {
        drain_commands(&mut service, &command_rx);
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        match service.loop_iteration() {
            Ok(outcome) => {
                let _ = service.record_lifecycle_observation(
                    LifecycleObservation::CaptureAvailabilityChanged {
                        state: CaptureAvailabilityState::Ready,
                        detected_by: "successful_loop".to_string(),
                    },
                );
                let _ = service.record_service_ping_if_due(
                    ServiceRole::PrimaryService,
                    "windows_resident_timer",
                );
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
                let sleep_duration = service
                    .next_service_ping_due_at_ms(ServiceRole::PrimaryService)
                    .ok()
                    .flatten()
                    .map(|due_at_ms| {
                        duration_until(due_at_ms.saturating_add(SERVICE_PING_WAKE_PADDING_MS))
                    })
                    .map(|ping_duration| ping_duration.min(duration_until(outcome.next_run_at_ms)))
                    .unwrap_or_else(|| duration_until(outcome.next_run_at_ms));

                wait_for_commands(&mut service, &command_rx, &shutdown, sleep_duration);
            }
            Err(err) => {
                let message = err.to_string();
                if is_capture_unavailable_error(&message) {
                    let _ = service.record_lifecycle_observation(
                        LifecycleObservation::CaptureAvailabilityChanged {
                            state: CaptureAvailabilityState::Blocked,
                            detected_by: "capture_error".to_string(),
                        },
                    );
                }
                update_snapshot(|snapshot| {
                    snapshot.state = "error".to_string();
                    snapshot.last_error = Some(message);
                });
                wait_for_commands(&mut service, &command_rx, &shutdown, ERROR_RETRY_INTERVAL);
            }
        }
    }

    let _ = service.shutdown();
}

fn handle_command(service: &mut MonitorService<WindowsPlatformHooks>, command: MonitorCommand) {
    match command {
        MonitorCommand::NoteStopRequested { source } => {
            let _ = service.note_stop_requested_by_user(ServiceRole::PrimaryService, &source);
        }
        MonitorCommand::ServiceStopObserved {
            raw_reason,
            shutdown_in_progress,
            explicit_user_stop,
            detected_by,
        } => {
            let _ =
                service.record_lifecycle_observation(LifecycleObservation::ServiceStopObserved {
                    role: ServiceRole::PrimaryService,
                    raw_reason,
                    shutdown_in_progress,
                    explicit_user_stop,
                    detected_by,
                });
        }
        MonitorCommand::UserSessionChanged {
            state,
            origin,
            detected_by,
        } => {
            let _ =
                service.record_lifecycle_observation(LifecycleObservation::UserSessionChanged {
                    state,
                    origin,
                    detected_by,
                });
        }
        MonitorCommand::ComputerPowerChanged {
            state,
            origin,
            detected_by,
            confidence,
        } => {
            let _ =
                service.record_lifecycle_observation(LifecycleObservation::ComputerPowerChanged {
                    state,
                    origin,
                    detected_by,
                    confidence,
                });
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

fn is_capture_unavailable_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    [
        "getdc failed",
        "createcompatibledc failed",
        "createcompatiblebitmap failed",
        "selectobject failed",
        "bitblt failed",
        "getdibits failed",
        "invalid screen size",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn current_boot_marker() -> Option<(String, i64)> {
    #[cfg(target_os = "windows")]
    {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| i64::try_from(duration.as_millis()).ok())?;
        let uptime_ms = i64::try_from(unsafe { GetTickCount64() }).ok()?;
        let booted_at_ms = now_ms.checked_sub(uptime_ms)?;
        Some((format!("windows_boot_{booted_at_ms}"), booted_at_ms))
    }

    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

fn duration_until(next_run_at_ms: i64) -> Duration {
    let now_ms = Utc::now().timestamp_millis();
    let delta_ms = next_run_at_ms.saturating_sub(now_ms);
    Duration::from_millis(delta_ms.max(0) as u64)
}

fn update_snapshot(update: impl FnOnce(&mut MonitorStatusSnapshot)) {
    let controller = controller();
    let mut state = controller.state.lock().expect("monitor controller lock");
    update(&mut state.snapshot);
}
