#![cfg(target_os = "windows")]

use std::ffi::OsString;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use tokio::runtime::Builder;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::w;

use windows_service::define_windows_service;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;

use virtue_windows_client::config::ClientPaths;
use virtue_windows_client::daemon;
use virtue_windows_client::runtime_env::apply_runtime_env;
use virtue_windows_client::service_log::ServiceLogger;

const SERVICE_NAME: &str = "VirtueCaptureService";
const CONSOLE_INSTANCE_MUTEX_NAME: windows::core::PCWSTR = w!("Local\\VirtueCaptureConsole");

define_windows_service!(ffi_service_main, service_main);

fn main() -> Result<()> {
    if std::env::args().any(|arg| arg == "--console") {
        return run_console();
    }

    if let Err(err) = service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
        eprintln!("service dispatcher start failed ({err}); falling back to --console mode");
        return run_console();
    }

    Ok(())
}

fn service_main(_arguments: Vec<OsString>) {
    let _ = run_service();
}

fn run_service() -> Result<()> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;
    apply_runtime_env(&paths);
    let logger = ServiceLogger::new(paths.log_file.clone());

    let shutdown = Arc::new(AtomicBool::new(false));
    let stop_signal = shutdown.clone();

    let status_handle =
        service_control_handler::register(SERVICE_NAME, move |event| match event {
            ServiceControl::Stop => {
                stop_signal.store(true, Ordering::SeqCst);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        })?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    })?;

    logger.info("windows service started");

    let runtime = Builder::new_multi_thread().enable_all().build()?;
    let daemon_result =
        runtime.block_on(async { daemon::run_daemon(shutdown.clone(), &logger).await });

    if let Err(err) = &daemon_result {
        logger.error(&format!("daemon failed: {err:#}"));
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

fn run_console() -> Result<()> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;
    apply_runtime_env(&paths);
    let logger = ServiceLogger::new(paths.log_file.clone());
    let shutdown = Arc::new(AtomicBool::new(false));
    let instance = acquire_console_instance_mutex()?;
    let Some(instance) = instance else {
        logger.info("console instance already running; exiting duplicate");
        return Ok(());
    };

    logger.info("running in console mode");

    let runtime = Builder::new_multi_thread().enable_all().build()?;
    let result = runtime.block_on(async {
        tokio::select! {
            result = daemon::run_daemon(shutdown.clone(), &logger) => {
                result
            }
            signal = tokio::signal::ctrl_c() => {
                match signal {
                    Ok(()) => {
                        shutdown.store(true, Ordering::SeqCst);
                        Ok(())
                    }
                    Err(err) => Err(anyhow::anyhow!("ctrl_c handler failed: {err}")),
                }
            }
        }
    });
    let _ = unsafe { CloseHandle(instance) };
    result
}

fn acquire_console_instance_mutex() -> Result<Option<HANDLE>> {
    unsafe {
        let handle = CreateMutexW(None, false, CONSOLE_INSTANCE_MUTEX_NAME)?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            return Ok(None);
        }
        Ok(Some(handle))
    }
}
