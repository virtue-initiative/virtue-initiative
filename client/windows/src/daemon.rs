use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use windows::Win32::System::SystemInformation::GetTickCount64;

use virtue_core::{
    ApiClient, AuthClient, BatchDaemonConfig, CaptureOutcome, CoreError, DaemonAlertEvent,
    FileTokenStore, PersistedServiceState, ServiceEvent, ServiceHost, SleepOutcome, TokenStore,
    run_batch_daemon,
};

use crate::capture_control;
use crate::config::{ClientPaths, load_state};
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

    fn should_emit_system_shutdown(self) -> bool {
        matches!(
            self,
            StopReason::ServiceControlShutdown
                | StopReason::ServiceControlPreshutdown
                | StopReason::ConsoleCtrlShutdown
        )
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct LifecycleState {
    last_seen_boot_id: Option<String>,
    daemon_running: bool,
    last_stop_signal: Option<String>,
    last_stop_at: Option<DateTime<Utc>>,
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

pub fn push_alert_event(
    pending_alert_events: &Arc<Mutex<Vec<DaemonAlertEvent>>>,
    event: DaemonAlertEvent,
    logger: &ServiceLogger,
) {
    match pending_alert_events.lock() {
        Ok(mut guard) => guard.push(event),
        Err(_) => logger.warn("could not record lifecycle event: alert event queue lock poisoned"),
    }
}

struct WindowsLifecycleHost<'a> {
    paths: ClientPaths,
    shutdown: Arc<AtomicBool>,
    stop_reason: Arc<Mutex<Option<StopReason>>>,
    logger: &'a ServiceLogger,
    stop_events_emitted: AtomicBool,
    capture_stop_signaled: AtomicBool,
    capture_supervisor: Mutex<CaptureSupervisorState>,
    pending_alert_events: Arc<Mutex<Vec<DaemonAlertEvent>>>,
}

impl<'a> WindowsLifecycleHost<'a> {
    fn new(
        paths: ClientPaths,
        shutdown: Arc<AtomicBool>,
        stop_reason: Arc<Mutex<Option<StopReason>>>,
        logger: &'a ServiceLogger,
        pending_alert_events: Arc<Mutex<Vec<DaemonAlertEvent>>>,
    ) -> Self {
        Self {
            paths,
            shutdown,
            stop_reason,
            logger,
            stop_events_emitted: AtomicBool::new(false),
            capture_stop_signaled: AtomicBool::new(false),
            capture_supervisor: Mutex::new(CaptureSupervisorState::default()),
            pending_alert_events,
        }
    }

    fn maybe_record_stop_events(&self) {
        if self.stop_events_emitted.swap(true, Ordering::SeqCst) {
            return;
        }

        let reason = self
            .stop_reason
            .lock()
            .ok()
            .and_then(|guard| *guard)
            .unwrap_or(StopReason::ServiceControlStop);
        let stop_signal = reason.signal_name().to_string();
        let now = Utc::now();

        let mut guard = match self.pending_alert_events.lock() {
            Ok(guard) => guard,
            Err(_) => {
                self.logger
                    .warn("could not record stop events: alert event queue lock poisoned");
                return;
            }
        };

        guard.push(DaemonAlertEvent {
            kind: "service_stop".to_string(),
            metadata: vec![
                ("source".to_string(), "windows_service".to_string()),
                ("signal".to_string(), stop_signal.clone()),
            ],
            created_at: now,
            device_id: None,
        });

        guard.push(DaemonAlertEvent {
            kind: "daemon_stop_signal".to_string(),
            metadata: vec![("signal".to_string(), stop_signal.clone())],
            created_at: now,
            device_id: None,
        });

        if reason.should_emit_system_shutdown() {
            guard.push(DaemonAlertEvent {
                kind: "system_shutdown".to_string(),
                metadata: vec![
                    ("source".to_string(), "windows_system".to_string()),
                    ("signal".to_string(), stop_signal),
                ],
                created_at: now,
                device_id: None,
            });
        }
        drop(guard);

        mark_daemon_stopped(&self.paths.lifecycle_state_file, reason, self.logger);
    }

    fn record_alert(&self, kind: &str, metadata: Vec<(String, String)>) {
        push_alert_event(
            &self.pending_alert_events,
            DaemonAlertEvent {
                kind: kind.to_string(),
                metadata,
                created_at: Utc::now(),
                device_id: None,
            },
            self.logger,
        );
    }

    fn maybe_signal_capture_stop(&self) {
        if self.capture_stop_signaled.swap(true, Ordering::SeqCst) {
            return;
        }
        match capture_control::signal_capture_stop(&self.paths) {
            Ok(()) => {
                self.logger
                    .info("capture stop signal set by lifecycle service");
                self.record_alert(
                    "capture_process_stop_requested",
                    vec![("source".to_string(), "windows_service".to_string())],
                );
            }
            Err(err) => self
                .logger
                .warn(&format!("failed to signal capture stop event: {err:#}")),
        }
    }

    fn supervise_capture_process(&self) {
        if self.shutdown.load(Ordering::SeqCst) {
            return;
        }

        let now = Instant::now();
        let mut supervisor = match self.capture_supervisor.lock() {
            Ok(guard) => guard,
            Err(_) => {
                self.logger.warn("capture supervisor lock poisoned");
                return;
            }
        };

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
            self.record_alert(
                "capture_process_missing",
                vec![("source".to_string(), "windows_service".to_string())],
            );
            supervisor.missing_reported = true;
        }

        if let Some(last_restart) = supervisor.last_restart_attempt
            && now.duration_since(last_restart) < CAPTURE_RESTART_RETRY_INTERVAL
        {
            return;
        }
        supervisor.last_restart_attempt = Some(now);
        drop(supervisor);

        if let Err(err) = capture_control::clear_capture_stop_signal(&self.paths) {
            self.logger.warn(&format!(
                "failed to clear capture stop signal before restart: {err:#}"
            ));
            return;
        }

        match capture_control::launch_capture_in_active_session(&self.paths) {
            Ok(Some(pid)) => {
                self.logger.info(&format!(
                    "capture process restart requested by lifecycle service (pid {pid})"
                ));
                self.record_alert(
                    "capture_process_restart",
                    vec![
                        ("source".to_string(), "windows_service".to_string()),
                        ("pid".to_string(), pid.to_string()),
                    ],
                );
            }
            Ok(None) => {
                self.logger
                    .info("capture restart skipped; no active interactive session");
            }
            Err(err) => {
                self.logger.warn(&format!(
                    "capture restart failed from lifecycle service: {err:#}"
                ));
                self.record_alert(
                    "capture_process_restart_failed",
                    vec![
                        ("source".to_string(), "windows_service".to_string()),
                        ("error".to_string(), err.to_string()),
                    ],
                );
            }
        }
    }
}

impl ServiceHost for WindowsLifecycleHost<'_> {
    fn load_persisted_state(&self) -> virtue_core::CoreResult<PersistedServiceState> {
        let state =
            load_state(&self.paths.state_file).map_err(|e| CoreError::Platform(e.to_string()))?;
        Ok(PersistedServiceState {
            // Lifecycle service does not perform capture. It only flushes alert events.
            monitoring_enabled: false,
            device_id: state.device_id,
        })
    }

    fn now_utc(&self) -> chrono::DateTime<Utc> {
        Utc::now()
    }

    async fn sleep_interruptible(
        &self,
        duration: Duration,
    ) -> virtue_core::CoreResult<SleepOutcome> {
        let mut remaining = duration;
        while remaining > Duration::ZERO {
            if self.should_stop() {
                return Ok(SleepOutcome::Interrupted);
            }
            let tick = remaining.min(Duration::from_secs(1));
            sleep(tick).await;
            remaining = remaining.saturating_sub(tick);
        }
        if self.should_stop() {
            Ok(SleepOutcome::Interrupted)
        } else {
            Ok(SleepOutcome::Elapsed)
        }
    }

    async fn capture_frame_png(&self) -> virtue_core::CoreResult<CaptureOutcome> {
        Ok(CaptureOutcome::SessionUnavailable)
    }

    fn emit_event(&self, event: ServiceEvent) {
        match event {
            ServiceEvent::Info(msg) => self.logger.info(&msg),
            ServiceEvent::Warn(msg) => self.logger.warn(&msg),
            ServiceEvent::Error(msg) => self.logger.error(&msg),
        }
    }

    fn should_stop(&self) -> bool {
        let stop = self.shutdown.load(Ordering::SeqCst);
        if stop {
            self.maybe_signal_capture_stop();
            self.maybe_record_stop_events();
        }
        stop
    }

    fn on_loop_tick(&self) -> virtue_core::CoreResult<()> {
        self.supervise_capture_process();
        Ok(())
    }

    fn drain_alert_events(&self) -> virtue_core::CoreResult<Vec<DaemonAlertEvent>> {
        let mut guard = self
            .pending_alert_events
            .lock()
            .map_err(|_| CoreError::Platform("alert event queue lock poisoned".to_string()))?;
        Ok(std::mem::take(&mut *guard))
    }
}

pub async fn run_daemon(
    shutdown: Arc<AtomicBool>,
    stop_reason: Arc<Mutex<Option<StopReason>>>,
    pending_alert_events: Arc<Mutex<Vec<DaemonAlertEvent>>>,
    logger: &ServiceLogger,
) -> Result<()> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;
    if let Err(err) = capture_control::clear_capture_stop_signal(&paths) {
        logger.warn(&format!(
            "failed to clear capture stop signal during lifecycle startup: {err:#}"
        ));
    }

    let mut startup_events = vec![
        DaemonAlertEvent {
            kind: "service_start".to_string(),
            metadata: vec![("source".to_string(), "windows_service".to_string())],
            created_at: Utc::now(),
            device_id: None,
        },
        DaemonAlertEvent {
            kind: "daemon_start".to_string(),
            metadata: vec![("source".to_string(), "windows_service".to_string())],
            created_at: Utc::now(),
            device_id: None,
        },
    ];
    startup_events.extend(collect_startup_lifecycle_events(&paths, logger));

    {
        let mut guard = pending_alert_events
            .lock()
            .map_err(|_| anyhow::anyhow!("alert event queue lock poisoned"))?;
        guard.extend(startup_events);
    }

    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let auth_client = AuthClient::new(token_store.clone())?;
    let api_client = ApiClient::new()?;
    let host = WindowsLifecycleHost::new(
        paths.clone(),
        shutdown,
        stop_reason,
        logger,
        pending_alert_events,
    );

    let config = BatchDaemonConfig {
        settings_refresh_interval: Duration::from_secs(30 * 60),
        settings_fetch_retry_interval: Duration::from_secs(20),
        idle_retry_interval: Duration::from_secs(15),
        token_refresh_threshold: Duration::from_secs(120),
        session_unavailable_log_interval: Duration::from_secs(5 * 60),
        continue_on_token_refresh_error: true,
    };

    run_batch_daemon(
        &host,
        token_store,
        &auth_client,
        &api_client,
        &paths.batch_buffer_file,
        config,
    )
    .await
    .map_err(Into::into)
}

fn collect_startup_lifecycle_events(
    paths: &ClientPaths,
    logger: &ServiceLogger,
) -> Vec<DaemonAlertEvent> {
    let Some((boot_id, started_at)) = read_current_boot_id() else {
        return Vec::new();
    };
    let mut state = load_lifecycle_state(&paths.lifecycle_state_file);
    let mut events = Vec::new();

    let boot_changed = state.last_seen_boot_id.as_deref() != Some(boot_id.as_str());
    if boot_changed && state.daemon_running {
        let event_log_shutdown_time = query_last_shutdown_time();
        let recovered_shutdown_time = event_log_shutdown_time.unwrap_or(started_at);
        let detected_by = if event_log_shutdown_time.is_some() {
            "windows_event_log_kernel_general_13"
        } else {
            "next_boot_recovery"
        };

        events.push(DaemonAlertEvent {
            kind: "service_stop".to_string(),
            metadata: vec![
                ("source".to_string(), "windows_system".to_string()),
                ("signal".to_string(), "BOOT_TRANSITION".to_string()),
                ("detected_by".to_string(), detected_by.to_string()),
            ],
            created_at: recovered_shutdown_time,
            device_id: None,
        });

        events.push(DaemonAlertEvent {
            kind: "daemon_stop_signal".to_string(),
            metadata: vec![
                ("signal".to_string(), "BOOT_TRANSITION".to_string()),
                ("source".to_string(), "windows_system".to_string()),
                ("detected_by".to_string(), detected_by.to_string()),
            ],
            created_at: recovered_shutdown_time,
            device_id: None,
        });

        events.push(DaemonAlertEvent {
            kind: "system_shutdown".to_string(),
            metadata: vec![
                ("source".to_string(), "windows_system".to_string()),
                ("signal".to_string(), "BOOT_TRANSITION".to_string()),
                ("detected_by".to_string(), detected_by.to_string()),
            ],
            created_at: recovered_shutdown_time,
            device_id: None,
        });
    }

    if boot_changed {
        events.push(DaemonAlertEvent {
            kind: "system_startup".to_string(),
            metadata: vec![
                ("source".to_string(), "windows_system".to_string()),
                ("boot_id".to_string(), boot_id.clone()),
                ("detected_by".to_string(), "boot_time_change".to_string()),
            ],
            created_at: started_at,
            device_id: None,
        });
    }

    state.last_seen_boot_id = Some(boot_id);
    state.daemon_running = true;
    state.last_stop_signal = None;
    state.last_stop_at = None;
    if let Err(err) = save_lifecycle_state(&paths.lifecycle_state_file, &state) {
        logger.warn(&format!(
            "could not persist lifecycle state {}: {err}",
            paths.lifecycle_state_file.display()
        ));
    }

    events
}

fn read_current_boot_id() -> Option<(String, DateTime<Utc>)> {
    let uptime_seconds = unsafe { GetTickCount64() } / 1000;
    let uptime_seconds = i64::try_from(uptime_seconds).ok()?;
    let boot_unix = Utc::now().timestamp().saturating_sub(uptime_seconds);
    let boot_time = DateTime::<Utc>::from_timestamp(boot_unix, 0)?;
    Some((format!("boot_ts_{boot_unix}"), boot_time))
}

fn query_last_shutdown_time() -> Option<DateTime<Utc>> {
    let output = Command::new("wevtutil")
        .args([
            "qe",
            "System",
            "/q:*[System[Provider[@Name='Microsoft-Windows-Kernel-General'] and (EventID=13)]]",
            "/rd:true",
            "/c:1",
            "/f:xml",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let xml = String::from_utf8_lossy(&output.stdout);
    let system_time = extract_system_time_attr(&xml)?;
    DateTime::parse_from_rfc3339(&system_time)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn extract_system_time_attr(xml: &str) -> Option<String> {
    for marker in ["SystemTime='", "SystemTime=\""] {
        if let Some(start) = xml.find(marker) {
            let value_start = start + marker.len();
            let rest = xml.get(value_start..)?;
            let end_ch = if marker.ends_with('"') { '"' } else { '\'' };
            let end = rest.find(end_ch)?;
            return Some(rest[..end].to_string());
        }
    }
    None
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

fn mark_daemon_stopped(path: &Path, reason: StopReason, logger: &ServiceLogger) {
    let mut state = load_lifecycle_state(path);
    state.daemon_running = false;
    state.last_stop_signal = Some(reason.signal_name().to_string());
    state.last_stop_at = Some(Utc::now());
    if let Err(err) = save_lifecycle_state(path, &state) {
        logger.warn(&format!(
            "could not persist lifecycle stop state {}: {err}",
            path.display()
        ));
    }
}
