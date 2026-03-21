use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::Utc;
use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, HANDLE};
use windows::Win32::System::Threading::{MUTEX_MODIFY_STATE, OpenMutexW};
use windows::core::w;

use virtue_core::MonitorService;

use crate::capture::WindowsPlatformHooks;
use crate::capture_control;
use crate::config::{ClientPaths, build_core_config};
use crate::service_log::ServiceLogger;

const ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(20);
const TRAY_ENSURE_INTERVAL: Duration = Duration::from_secs(30);

pub fn run_daemon(shutdown: Arc<AtomicBool>, logger: &ServiceLogger) -> Result<()> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;

    let mut service =
        MonitorService::setup(build_core_config(&paths), WindowsPlatformHooks::new())?;
    let mut last_tray_ensure: Option<Instant> = None;

    loop {
        if should_stop(&shutdown, &paths) {
            break;
        }

        if last_tray_ensure.is_none_or(|instant| instant.elapsed() >= TRAY_ENSURE_INTERVAL) {
            ensure_tray_running(logger);
            last_tray_ensure = Some(Instant::now());
        }

        let sleep_duration = match service.loop_iteration() {
            Ok(outcome) => duration_until(outcome.next_run_at_ms),
            Err(err) => {
                logger.error(&format!("capture loop failed: {err}"));
                ERROR_RETRY_INTERVAL
            }
        };

        sleep_interruptible(&shutdown, &paths, sleep_duration);
    }

    let _ = service.shutdown();
    Ok(())
}

fn should_stop(shutdown: &Arc<AtomicBool>, paths: &ClientPaths) -> bool {
    shutdown.load(Ordering::SeqCst) || capture_control::is_capture_stop_requested(paths)
}

fn sleep_interruptible(shutdown: &Arc<AtomicBool>, paths: &ClientPaths, duration: Duration) {
    let mut remaining = duration;
    while remaining > Duration::ZERO && !should_stop(shutdown, paths) {
        let tick = remaining.min(Duration::from_secs(1));
        thread::sleep(tick);
        remaining = remaining.saturating_sub(tick);
    }
}

fn duration_until(next_run_at_ms: i64) -> Duration {
    let now_ms = Utc::now().timestamp_millis();
    let delta_ms = next_run_at_ms.saturating_sub(now_ms);
    Duration::from_millis(delta_ms.max(0) as u64)
}

fn ensure_tray_running(logger: &ServiceLogger) {
    if is_tray_running() {
        return;
    }

    let tray_path = match std::env::current_exe() {
        Ok(path) => path.with_file_name("virtue-tray.exe"),
        Err(err) => {
            logger.warn(&format!("cannot resolve tray path from current exe: {err}"));
            return;
        }
    };

    if !tray_path.exists() {
        logger.warn(&format!("tray executable missing: {}", tray_path.display()));
        return;
    }

    match Command::new(&tray_path).spawn() {
        Ok(_) => logger.info("tray process launch requested by capture daemon"),
        Err(err) => logger.warn(&format!("failed to launch tray process: {err}")),
    }
}

fn is_tray_running() -> bool {
    unsafe {
        let handle: Result<HANDLE, _> =
            OpenMutexW(MUTEX_MODIFY_STATE, false, w!("Local\\VirtueTrayInstance"));
        match handle {
            Ok(handle) => {
                let _ = CloseHandle(handle);
                true
            }
            Err(err) => err.code() != ERROR_FILE_NOT_FOUND.into(),
        }
    }
}
