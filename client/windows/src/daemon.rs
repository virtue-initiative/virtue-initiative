use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use virtue_core::{CoreError, CoreResult, LogEntry, MonitorService, PlatformHooks, Screenshot};
use windows::Win32::System::SystemInformation::GetTickCount64;

use crate::capture_control;
use crate::config::{ClientPaths, build_core_config};
use crate::service_log::ServiceLogger;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopReason {
    ServiceControlStop,
    ServiceControlShutdown,
    ServiceControlPreshutdown,
    ConsoleCtrlC,
    ConsoleCtrlBreak,
    ConsoleCtrlClose,
    ConsoleCtrlLogoff,
    ConsoleCtrlShutdown,
}

impl StopReason {
    pub fn signal_name(self) -> &'static str {
        match self {
            StopReason::ServiceControlStop => "SERVICE_CONTROL_STOP",
            StopReason::ServiceControlShutdown => "SERVICE_CONTROL_SHUTDOWN",
            StopReason::ServiceControlPreshutdown => "SERVICE_CONTROL_PRESHUTDOWN",
            StopReason::ConsoleCtrlC => "CTRL_C_EVENT",
            StopReason::ConsoleCtrlBreak => "CTRL_BREAK_EVENT",
            StopReason::ConsoleCtrlClose => "CTRL_CLOSE_EVENT",
            StopReason::ConsoleCtrlLogoff => "CTRL_LOGOFF_EVENT",
            StopReason::ConsoleCtrlShutdown => "CTRL_SHUTDOWN_EVENT",
        }
    }

    pub fn should_emit_system_shutdown(self) -> bool {
        matches!(
            self,
            StopReason::ServiceControlShutdown
                | StopReason::ServiceControlPreshutdown
                | StopReason::ConsoleCtrlShutdown
        )
    }
}

const CAPTURE_SUPERVISOR_INTERVAL: Duration = Duration::from_secs(10);
const CAPTURE_RESTART_RETRY_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Default, Serialize, Deserialize)]
struct LifecycleState {
    last_seen_boot_id: Option<String>,
}

#[derive(Debug, Default)]
struct CaptureSupervisorState {
    last_check: Option<Instant>,
    last_restart_attempt: Option<Instant>,
    saw_running_once: bool,
    missing_reported: bool,
}

#[derive(Clone)]
struct LifecyclePlatformHooks;

impl PlatformHooks for LifecyclePlatformHooks {
    fn take_screenshot(&self) -> CoreResult<Screenshot> {
        Err(CoreError::CommandFailed(
            "capture is unavailable in lifecycle service".to_string(),
        ))
    }

    fn get_time_utc_ms(&self) -> CoreResult<i64> {
        let duration = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|err| CoreError::CommandFailed(err.to_string()))?;
        i64::try_from(duration.as_millis())
            .map_err(|_| CoreError::InvalidState("system clock overflow"))
    }
}

pub fn run_daemon(
    shutdown: Arc<AtomicBool>,
    stop_reason: Arc<Mutex<Option<StopReason>>>,
    logger: &ServiceLogger,
) -> Result<()> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;

    emit_log(
        &paths,
        logger,
        "daemon_start",
        &[("source", "windows_service")],
    );
    if let Some(startup_event) = detect_system_startup_event(&paths) {
        emit_log_entry(&paths, logger, startup_event);
    }

    let mut supervisor = CaptureSupervisorState::default();
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        supervise_capture_process(&paths, logger, &mut supervisor);
        sleep_interruptible(&shutdown, Duration::from_secs(1));
    }

    let reason = stop_reason
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .unwrap_or(StopReason::ServiceControlStop);

    emit_log(
        &paths,
        logger,
        "daemon_stop_signal",
        &[("signal", reason.signal_name())],
    );
    if reason.should_emit_system_shutdown() {
        emit_log(
            &paths,
            logger,
            "system_shutdown",
            &[
                ("source", "windows_system"),
                ("signal", reason.signal_name()),
            ],
        );
    }

    Ok(())
}

pub fn set_stop_reason_if_empty(target: &Arc<Mutex<Option<StopReason>>>, reason: StopReason) {
    if let Ok(mut guard) = target.lock()
        && guard.is_none()
    {
        *guard = Some(reason);
    }
}

fn detect_system_startup_event(paths: &ClientPaths) -> Option<LogEntry> {
    let (boot_id, started_at) = current_boot_marker()?;
    let mut state = load_lifecycle_state(&paths.lifecycle_state_file);
    if state.last_seen_boot_id.as_deref() == Some(boot_id.as_str()) {
        return None;
    }

    state.last_seen_boot_id = Some(boot_id.clone());
    if let Err(err) = save_lifecycle_state(&paths.lifecycle_state_file, &state) {
        eprintln!(
            "daemon: could not persist lifecycle state {}: {err}",
            paths.lifecycle_state_file.display()
        );
    }

    Some(LogEntry {
        ts_ms: started_at.timestamp_millis(),
        kind: "system_startup".to_string(),
        risk: None,
        data: metadata_value(&[
            ("source", "windows_system"),
            ("boot_id", boot_id.as_str()),
            ("detected_by", "boot_time_change"),
        ]),
    })
}

fn emit_log(paths: &ClientPaths, logger: &ServiceLogger, kind: &str, metadata: &[(&str, &str)]) {
    emit_log_entry(
        paths,
        logger,
        LogEntry {
            ts_ms: Utc::now().timestamp_millis(),
            kind: kind.to_string(),
            risk: None,
            data: metadata_value(metadata),
        },
    );
}

fn emit_log_entry(paths: &ClientPaths, logger: &ServiceLogger, entry: LogEntry) {
    let mut service = match MonitorService::setup(build_core_config(paths), LifecyclePlatformHooks)
    {
        Ok(service) => service,
        Err(err) => {
            logger.warn(&format!("failed to prepare lifecycle logger: {err:#}"));
            return;
        }
    };

    let kind = entry.kind.clone();
    if let Err(err) = service.send_log(entry) {
        logger.warn(&format!("failed to queue lifecycle log {kind}: {err:#}"));
    }
}

fn metadata_value(metadata: &[(&str, &str)]) -> serde_json::Value {
    serde_json::Value::Object(
        metadata
            .iter()
            .map(|(key, value)| {
                (
                    (*key).to_string(),
                    serde_json::Value::String((*value).to_string()),
                )
            })
            .collect::<serde_json::Map<_, _>>(),
    )
}

fn current_boot_marker() -> Option<(String, DateTime<Utc>)> {
    let now = Utc::now();
    let uptime_ms = unsafe { GetTickCount64() };
    let uptime_ms = i64::try_from(uptime_ms).ok()?;
    let started_at = now - chrono::TimeDelta::milliseconds(uptime_ms);
    let boot_id = started_at.timestamp_millis().to_string();
    Some((boot_id, started_at))
}

fn load_lifecycle_state(path: &Path) -> LifecycleState {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LifecycleState>(&bytes).ok())
        .unwrap_or_default()
}

fn save_lifecycle_state(path: &Path, state: &LifecycleState) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    fs::create_dir_all(parent)?;
    let tmp_path = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(state)?;
    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn supervise_capture_process(
    paths: &ClientPaths,
    logger: &ServiceLogger,
    supervisor: &mut CaptureSupervisorState,
) {
    let now = Instant::now();
    if let Some(last) = supervisor.last_check
        && now.duration_since(last) < CAPTURE_SUPERVISOR_INTERVAL
    {
        return;
    }
    supervisor.last_check = Some(now);

    if capture_control::is_capture_running() {
        supervisor.saw_running_once = true;
        supervisor.missing_reported = false;
        return;
    }

    if supervisor.saw_running_once && !supervisor.missing_reported {
        logger.warn("capture process missing");
        supervisor.missing_reported = true;
    }

    if let Some(last_restart) = supervisor.last_restart_attempt
        && now.duration_since(last_restart) < CAPTURE_RESTART_RETRY_INTERVAL
    {
        return;
    }
    supervisor.last_restart_attempt = Some(now);

    if let Err(err) = capture_control::clear_capture_stop_signal(paths) {
        logger.warn(&format!(
            "failed to clear capture stop signal before restart: {err:#}"
        ));
        return;
    }

    match capture_control::launch_capture_in_active_session(paths) {
        Ok(Some(pid)) => logger.info(&format!(
            "capture process restart requested by lifecycle service (pid {pid})"
        )),
        Ok(None) => logger.info("capture restart skipped; no active interactive session"),
        Err(err) => logger.warn(&format!(
            "capture restart failed from lifecycle service: {err:#}"
        )),
    }
}

fn sleep_interruptible(shutdown: &Arc<AtomicBool>, duration: Duration) {
    let mut remaining = duration;
    while remaining > Duration::ZERO && !shutdown.load(Ordering::SeqCst) {
        let tick = remaining.min(Duration::from_secs(1));
        thread::sleep(tick);
        remaining = remaining.saturating_sub(tick);
    }
}
