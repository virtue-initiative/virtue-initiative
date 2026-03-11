#![cfg(target_os = "windows")]

use std::ffi::OsString;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use chrono::Utc;
use tokio::runtime::Builder;
use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close, ctrl_logoff, ctrl_shutdown};
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::w;

use windows_service::define_windows_service;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
    ServiceType, SessionChangeReason,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;

use virtue_client_core::DaemonAlertEvent;
use virtue_windows_client::capture_control;
use virtue_windows_client::capture_daemon;
use virtue_windows_client::config::ClientPaths;
use virtue_windows_client::daemon;
use virtue_windows_client::runtime_env::apply_runtime_env;
use virtue_windows_client::service_log::ServiceLogger;

const SERVICE_NAME: &str = "VirtueLifecycleService";
const LIFECYCLE_INSTANCE_MUTEX_NAME: windows::core::PCWSTR = w!("Local\\VirtueLifecycleConsole");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Capture,
    Lifecycle,
}

define_windows_service!(ffi_service_main, service_main);

fn main() -> Result<()> {
    let mode = parse_mode();
    let force_console = has_flag("--console");

    match mode {
        Some(Mode::Capture) => run_capture_console(),
        Some(Mode::Lifecycle) => {
            if force_console {
                run_lifecycle_console()
            } else if let Err(err) = service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
                eprintln!(
                    "service dispatcher start failed ({err}); falling back to lifecycle console mode"
                );
                run_lifecycle_console()
            } else {
                Ok(())
            }
        }
        None => {
            if force_console {
                return run_capture_console();
            }
            if let Err(err) = service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
                eprintln!(
                    "service dispatcher start failed ({err}); falling back to capture console mode"
                );
                run_capture_console()
            } else {
                Ok(())
            }
        }
    }
}

fn parse_mode() -> Option<Mode> {
    let args: Vec<String> = std::env::args().collect();
    for (i, arg) in args.iter().enumerate() {
        if let Some(value) = arg.strip_prefix("--mode=") {
            return Some(parse_mode_value(value));
        }
        if arg == "--mode"
            && let Some(value) = args.get(i + 1)
        {
            return Some(parse_mode_value(value));
        }
    }
    None
}

fn parse_mode_value(value: &str) -> Mode {
    match value.trim().to_ascii_lowercase().as_str() {
        "lifecycle" => Mode::Lifecycle,
        _ => Mode::Capture,
    }
}

fn has_flag(flag: &str) -> bool {
    std::env::args().any(|arg| arg == flag)
}

fn service_main(_arguments: Vec<OsString>) {
    let _ = run_lifecycle_service();
}

fn run_lifecycle_service() -> Result<()> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;
    apply_runtime_env(&paths);
    let logger = Arc::new(ServiceLogger::new(paths.log_file.clone()));
    logger.info(&format!("build {}", virtue_client_core::BUILD_LABEL));

    let shutdown = Arc::new(AtomicBool::new(false));
    let stop_signal = shutdown.clone();
    let stop_reason = Arc::new(Mutex::new(None));
    let stop_reason_for_handler = stop_reason.clone();
    let pending_alert_events = Arc::new(Mutex::new(Vec::<DaemonAlertEvent>::new()));
    let pending_alert_events_for_handler = pending_alert_events.clone();
    let logger_for_handler = logger.clone();

    let status_handle =
        service_control_handler::register(SERVICE_NAME, move |event| match event {
            ServiceControl::Stop => {
                set_stop_reason_if_empty(
                    &stop_reason_for_handler,
                    daemon::StopReason::ServiceControlStop,
                );
                stop_signal.store(true, Ordering::SeqCst);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Preshutdown => {
                set_stop_reason_if_empty(
                    &stop_reason_for_handler,
                    daemon::StopReason::ServiceControlPreshutdown,
                );
                stop_signal.store(true, Ordering::SeqCst);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Shutdown => {
                set_stop_reason_if_empty(
                    &stop_reason_for_handler,
                    daemon::StopReason::ServiceControlShutdown,
                );
                stop_signal.store(true, Ordering::SeqCst);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::SessionChange(param) => {
                if let Some(alert) =
                    map_session_change_alert(param.reason, param.notification.session_id)
                {
                    daemon::push_alert_event(
                        &pending_alert_events_for_handler,
                        alert,
                        &logger_for_handler,
                    );
                }
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        })?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP
            | ServiceControlAccept::PRESHUTDOWN
            | ServiceControlAccept::SESSION_CHANGE,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    })?;

    logger.info("lifecycle service started");

    let runtime = Builder::new_multi_thread().enable_all().build()?;
    let daemon_result = runtime.block_on(async {
        daemon::run_daemon(
            shutdown.clone(),
            stop_reason.clone(),
            pending_alert_events.clone(),
            logger.as_ref(),
        )
        .await
    });

    if let Err(err) = &daemon_result {
        logger.error(&format!("lifecycle daemon failed: {err:#}"));
    }

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    })?;

    daemon_result
}

fn run_lifecycle_console() -> Result<()> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;
    apply_runtime_env(&paths);
    let logger = Arc::new(ServiceLogger::new(paths.log_file.clone()));
    logger.info(&format!("build {}", virtue_client_core::BUILD_LABEL));
    let shutdown = Arc::new(AtomicBool::new(false));
    let stop_reason = Arc::new(Mutex::new(None));
    let pending_alert_events = Arc::new(Mutex::new(Vec::<DaemonAlertEvent>::new()));
    let instance = acquire_console_instance_mutex(LIFECYCLE_INSTANCE_MUTEX_NAME)?;
    let Some(instance) = instance else {
        logger.info("lifecycle console instance already running; exiting duplicate");
        return Ok(());
    };

    logger.info("running lifecycle in console mode");

    let runtime = Builder::new_multi_thread().enable_all().build()?;
    let result = runtime.block_on(async {
        let shutdown_for_signals = shutdown.clone();
        let stop_reason_for_signals = stop_reason.clone();
        let signal_task = tokio::spawn(async move {
            let mut sig_ctrl_c = match ctrl_c() {
                Ok(sig) => sig,
                Err(_) => return,
            };
            let mut sig_ctrl_break = match ctrl_break() {
                Ok(sig) => sig,
                Err(_) => return,
            };
            let mut sig_ctrl_close = match ctrl_close() {
                Ok(sig) => sig,
                Err(_) => return,
            };
            let mut sig_ctrl_logoff = match ctrl_logoff() {
                Ok(sig) => sig,
                Err(_) => return,
            };
            let mut sig_ctrl_shutdown = match ctrl_shutdown() {
                Ok(sig) => sig,
                Err(_) => return,
            };

            tokio::select! {
                _ = sig_ctrl_c.recv() => {
                    set_stop_reason_if_empty(&stop_reason_for_signals, daemon::StopReason::ConsoleCtrlC);
                }
                _ = sig_ctrl_break.recv() => {
                    set_stop_reason_if_empty(&stop_reason_for_signals, daemon::StopReason::ConsoleCtrlBreak);
                }
                _ = sig_ctrl_close.recv() => {
                    set_stop_reason_if_empty(&stop_reason_for_signals, daemon::StopReason::ConsoleCtrlClose);
                }
                _ = sig_ctrl_logoff.recv() => {
                    set_stop_reason_if_empty(&stop_reason_for_signals, daemon::StopReason::ConsoleCtrlLogoff);
                }
                _ = sig_ctrl_shutdown.recv() => {
                    set_stop_reason_if_empty(&stop_reason_for_signals, daemon::StopReason::ConsoleCtrlShutdown);
                }
            }
            shutdown_for_signals.store(true, Ordering::SeqCst);
        });

        let daemon_result = daemon::run_daemon(
            shutdown.clone(),
            stop_reason.clone(),
            pending_alert_events.clone(),
            logger.as_ref(),
        )
        .await;

        signal_task.abort();
        let _ = signal_task.await;
        daemon_result
    });

    let _ = unsafe { CloseHandle(instance) };
    result
}

fn run_capture_console() -> Result<()> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;
    apply_runtime_env(&paths);
    let logger = Arc::new(ServiceLogger::new(paths.log_file.clone()));
    logger.info(&format!("build {}", virtue_client_core::BUILD_LABEL));
    let shutdown = Arc::new(AtomicBool::new(false));
    let instance = acquire_console_instance_mutex(capture_control::CAPTURE_INSTANCE_MUTEX_NAME)?;
    let Some(instance) = instance else {
        logger.info("capture console instance already running; exiting duplicate");
        return Ok(());
    };

    logger.info("running capture in console mode");

    let runtime = Builder::new_multi_thread().enable_all().build()?;
    let result = runtime.block_on(async {
        let shutdown_for_signals = shutdown.clone();
        let signal_task = tokio::spawn(async move {
            let mut sig_ctrl_c = match ctrl_c() {
                Ok(sig) => sig,
                Err(_) => return,
            };
            let mut sig_ctrl_break = match ctrl_break() {
                Ok(sig) => sig,
                Err(_) => return,
            };
            let mut sig_ctrl_close = match ctrl_close() {
                Ok(sig) => sig,
                Err(_) => return,
            };
            let mut sig_ctrl_logoff = match ctrl_logoff() {
                Ok(sig) => sig,
                Err(_) => return,
            };
            let mut sig_ctrl_shutdown = match ctrl_shutdown() {
                Ok(sig) => sig,
                Err(_) => return,
            };

            tokio::select! {
                _ = sig_ctrl_c.recv() => {}
                _ = sig_ctrl_break.recv() => {}
                _ = sig_ctrl_close.recv() => {}
                _ = sig_ctrl_logoff.recv() => {}
                _ = sig_ctrl_shutdown.recv() => {}
            }
            shutdown_for_signals.store(true, Ordering::SeqCst);
        });

        let daemon_result = capture_daemon::run_daemon(shutdown.clone(), logger.as_ref()).await;

        signal_task.abort();
        let _ = signal_task.await;
        daemon_result
    });

    let _ = unsafe { CloseHandle(instance) };
    result
}

fn map_session_change_alert(
    reason: SessionChangeReason,
    session_id: u32,
) -> Option<DaemonAlertEvent> {
    let kind = match reason {
        SessionChangeReason::SessionLogon => "session_login",
        SessionChangeReason::SessionLogoff => "session_logout",
        _ => return None,
    };

    Some(DaemonAlertEvent {
        kind: kind.to_string(),
        metadata: vec![
            ("source".to_string(), "windows_service".to_string()),
            ("session_id".to_string(), session_id.to_string()),
        ],
        created_at: Utc::now(),
        device_id: None,
    })
}

fn set_stop_reason_if_empty(
    stop_reason: &Arc<Mutex<Option<daemon::StopReason>>>,
    reason: daemon::StopReason,
) {
    if let Ok(mut guard) = stop_reason.lock()
        && guard.is_none()
    {
        *guard = Some(reason);
    }
}

fn acquire_console_instance_mutex(name: windows::core::PCWSTR) -> Result<Option<HANDLE>> {
    unsafe {
        let handle = CreateMutexW(None, false, name)?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            return Ok(None);
        }
        Ok(Some(handle))
    }
}
