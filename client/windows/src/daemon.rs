use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Utc};
use virtue_core::{
    CoreError, CoreResult, LifecycleObservation, MonitorService, PlatformHooks, Screenshot,
    ServiceRole,
};
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

    let mut service = MonitorService::setup(build_core_config(&paths), LifecyclePlatformHooks)?;
    if let Some((boot_marker, _)) = current_boot_marker() {
        let _ = service.record_lifecycle_observation(LifecycleObservation::BootObserved {
            boot_marker,
            booted_at_ms: None,
            detected_by: "boot_time_change".to_string(),
        });
    }
    let _ = service.record_lifecycle_observation(LifecycleObservation::ServiceStarted {
        role: ServiceRole::PrimaryService,
        detected_by: "windows_service".to_string(),
    });

    let mut supervisor = CaptureSupervisorState::default();
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        supervise_capture_process(&paths, logger, &mut supervisor, &mut service);
        sleep_interruptible(&shutdown, Duration::from_secs(1));
    }

    let reason = stop_reason
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .unwrap_or(StopReason::ServiceControlStop);

    let _ = service.record_lifecycle_observation(LifecycleObservation::ServiceStopObserved {
        role: ServiceRole::PrimaryService,
        raw_reason: reason.signal_name().to_string(),
        shutdown_in_progress: reason.should_emit_system_shutdown(),
        explicit_user_stop: false,
        detected_by: "windows_service_control".to_string(),
    });

    Ok(())
}

pub fn set_stop_reason_if_empty(target: &Arc<Mutex<Option<StopReason>>>, reason: StopReason) {
    if let Ok(mut guard) = target.lock()
        && guard.is_none()
    {
        *guard = Some(reason);
    }
}

fn current_boot_marker() -> Option<(String, DateTime<Utc>)> {
    let now = Utc::now();
    let uptime_ms = unsafe { GetTickCount64() };
    let uptime_ms = i64::try_from(uptime_ms).ok()?;
    let started_at = now - chrono::TimeDelta::milliseconds(uptime_ms);
    let boot_id = started_at.timestamp_millis().to_string();
    Some((boot_id, started_at))
}

fn supervise_capture_process(
    paths: &ClientPaths,
    logger: &ServiceLogger,
    supervisor: &mut CaptureSupervisorState,
    service: &mut MonitorService<LifecyclePlatformHooks>,
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
        let _ = service.record_lifecycle_observation(LifecycleObservation::ServiceStarted {
            role: ServiceRole::CaptureWorker,
            detected_by: "capture_supervisor".to_string(),
        });
        return;
    }

    if supervisor.saw_running_once && !supervisor.missing_reported {
        logger.warn("capture process missing");
        supervisor.missing_reported = true;
        let _ = service.record_lifecycle_observation(LifecycleObservation::ProcessMissing {
            role: ServiceRole::CaptureWorker,
            had_expected_runtime: true,
            detected_by: "capture_supervisor".to_string(),
        });
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
