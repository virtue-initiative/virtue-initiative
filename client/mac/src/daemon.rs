use std::fs;
use std::path::Path;
use std::process::Command;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use block2::RcBlock;
use chrono::{DateTime, Utc};
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSWorkspace, NSWorkspaceWillPowerOffNotification};
use objc2_foundation::NSNotification;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use virtue_client_core::{
    ApiClient, AuthClient, BatchDaemonConfig, CaptureOutcome, CoreError, DaemonAlertEvent,
    FileTokenStore, PersistedServiceState, ServiceEvent, ServiceHost, SleepOutcome, TokenStore,
    run_batch_daemon,
};

use crate::capture::{capture_screen, has_screen_capture_access, request_screen_capture_access};
use crate::config::{
    ClientPaths, ScreenshotPermissionStatus, load_daemon_status, load_state, save_daemon_status,
};

#[derive(Clone)]
struct MacDaemonHost {
    paths: ClientPaths,
    shutdown: Arc<AtomicBool>,
    permission_prompt_requested: Arc<AtomicBool>,
    pending_alert_events: Arc<Mutex<Vec<DaemonAlertEvent>>>,
}

impl MacDaemonHost {
    fn new(
        paths: ClientPaths,
        shutdown: Arc<AtomicBool>,
        pending_alert_events: Arc<Mutex<Vec<DaemonAlertEvent>>>,
    ) -> Self {
        Self {
            paths,
            shutdown,
            permission_prompt_requested: Arc::new(AtomicBool::new(false)),
            pending_alert_events,
        }
    }
}

impl ServiceHost for MacDaemonHost {
    fn load_persisted_state(&self) -> virtue_client_core::CoreResult<PersistedServiceState> {
        let state =
            load_state(&self.paths.state_file).map_err(|e| CoreError::Platform(e.to_string()))?;
        Ok(PersistedServiceState {
            monitoring_enabled: state.monitoring_enabled,
            device_id: state.device_id,
        })
    }

    fn now_utc(&self) -> chrono::DateTime<Utc> {
        Utc::now()
    }

    async fn sleep_interruptible(
        &self,
        duration: Duration,
    ) -> virtue_client_core::CoreResult<SleepOutcome> {
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

    async fn capture_frame_png(&self) -> virtue_client_core::CoreResult<CaptureOutcome> {
        if !has_screen_capture_access() {
            if !self
                .permission_prompt_requested
                .swap(true, Ordering::SeqCst)
            {
                let _ = request_screen_capture_access();
            }
            let error_text = "screen recording permission missing for daemon process".to_string();
            update_daemon_status(
                &self.paths,
                ScreenshotPermissionStatus::Missing,
                Some(error_text),
            );
            return Ok(CaptureOutcome::PermissionMissing);
        }

        self.permission_prompt_requested
            .store(false, Ordering::SeqCst);

        match capture_screen() {
            Ok(bytes) => {
                update_daemon_status(&self.paths, ScreenshotPermissionStatus::Granted, None);
                Ok(CaptureOutcome::FramePng(bytes))
            }
            Err(err) => {
                let error_text = format!("{err:#}");
                let screenshot_permission = if is_permission_missing_error(&error_text) {
                    ScreenshotPermissionStatus::Missing
                } else {
                    ScreenshotPermissionStatus::Unknown
                };
                update_daemon_status(&self.paths, screenshot_permission, Some(error_text.clone()));
                if screenshot_permission == ScreenshotPermissionStatus::Missing {
                    Ok(CaptureOutcome::PermissionMissing)
                } else {
                    Err(CoreError::Platform(error_text))
                }
            }
        }
    }

    fn emit_event(&self, event: ServiceEvent) {
        match event {
            ServiceEvent::Info(msg) => eprintln!("daemon: {msg}"),
            ServiceEvent::Warn(msg) => eprintln!("daemon: {msg}"),
            ServiceEvent::Error(msg) => eprintln!("daemon: {msg}"),
        }
    }

    fn drain_alert_events(&self) -> virtue_client_core::CoreResult<Vec<DaemonAlertEvent>> {
        let mut guard = self
            .pending_alert_events
            .lock()
            .map_err(|_| CoreError::Platform("alert event queue lock poisoned".to_string()))?;
        Ok(std::mem::take(&mut *guard))
    }

    fn should_stop(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct LifecycleState {
    last_seen_boot_time_sec: Option<i64>,
    // Legacy field kept for backward-compatible parsing of old state files.
    last_seen_boot_session_uuid: Option<String>,
}

#[derive(Default)]
struct WorkspacePowerState {
    will_power_off_at_unix_ms: AtomicI64,
}

impl WorkspacePowerState {
    fn mark_power_off(&self, ts: DateTime<Utc>) {
        self.will_power_off_at_unix_ms
            .store(ts.timestamp_millis(), Ordering::SeqCst);
    }

    fn power_off_at(&self) -> Option<DateTime<Utc>> {
        let raw = self.will_power_off_at_unix_ms.load(Ordering::SeqCst);
        if raw <= 0 {
            None
        } else {
            DateTime::<Utc>::from_timestamp_millis(raw)
        }
    }
}

struct WorkspacePowerMonitor {
    state: Arc<WorkspacePowerState>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl WorkspacePowerMonitor {
    fn start() -> Self {
        let state = Arc::new(WorkspacePowerState::default());
        let stop = Arc::new(AtomicBool::new(false));

        let state_for_thread = state.clone();
        let stop_for_thread = stop.clone();
        let worker = thread::spawn(move || {
            autoreleasepool(|_| {
                let workspace = NSWorkspace::sharedWorkspace();
                let center = workspace.notificationCenter();

                let state_for_block = state_for_thread.clone();
                let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
                    state_for_block.mark_power_off(Utc::now());
                });

                let _observer = unsafe {
                    center.addObserverForName_object_queue_usingBlock(
                        Some(NSWorkspaceWillPowerOffNotification),
                        None,
                        None,
                        &block,
                    )
                };

                while !stop_for_thread.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(250));
                }
            });
        });

        Self {
            state,
            stop,
            worker: Some(worker),
        }
    }

    fn state(&self) -> Arc<WorkspacePowerState> {
        self.state.clone()
    }
}

impl Drop for WorkspacePowerMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub async fn run_daemon(paths: &ClientPaths) -> Result<()> {
    paths.ensure_dirs()?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let power_monitor = WorkspacePowerMonitor::start();
    let workspace_power_state = power_monitor.state();

    let mut startup_events = vec![DaemonAlertEvent {
        kind: "daemon_start".to_string(),
        metadata: vec![("source".to_string(), "macos_launch_agent".to_string())],
        created_at: Utc::now(),
        device_id: None,
    }];
    if let Some(system_startup_event) = detect_system_startup_event(paths) {
        startup_events.push(system_startup_event);
    }
    let pending_alert_events = Arc::new(Mutex::new(startup_events));

    {
        let shutdown = shutdown.clone();
        let pending_alert_events = pending_alert_events.clone();
        let workspace_power_state = workspace_power_state.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{SignalKind, signal};

            let mut sigterm = match signal(SignalKind::terminate()) {
                Ok(s) => s,
                Err(_) => return,
            };
            let mut sigint = signal(SignalKind::interrupt()).ok();

            let signal_name = tokio::select! {
                _ = sigterm.recv() => "SIGTERM",
                _ = async {
                    match sigint.as_mut() {
                        Some(s) => s.recv().await,
                        None => std::future::pending::<Option<()>>().await,
                    }
                } => "SIGINT",
            };

            let power_off_observed = workspace_power_state.power_off_at().is_some();
            if let Ok(mut guard) = pending_alert_events.lock() {
                guard.push(DaemonAlertEvent {
                    kind: "daemon_stop_signal".to_string(),
                    metadata: vec![
                        ("signal".to_string(), signal_name.to_string()),
                        (
                            "power_off_observed".to_string(),
                            power_off_observed.to_string(),
                        ),
                    ],
                    created_at: Utc::now(),
                    device_id: None,
                });

                if signal_name == "SIGTERM"
                    && let Some(power_off_at) = workspace_power_state.power_off_at()
                {
                    guard.push(DaemonAlertEvent {
                        kind: "system_shutdown".to_string(),
                        metadata: vec![
                            ("source".to_string(), "macos_workspace".to_string()),
                            (
                                "detected_by".to_string(),
                                "nsworkspace_will_power_off_notification".to_string(),
                            ),
                            ("signal".to_string(), signal_name.to_string()),
                        ],
                        created_at: power_off_at,
                        device_id: None,
                    });
                }
            }
            shutdown.store(true, Ordering::SeqCst);
        });
    }

    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let auth_client = AuthClient::new(token_store.clone())?;
    let api_client = ApiClient::new()?;
    let host = MacDaemonHost::new(paths.clone(), shutdown, pending_alert_events);

    let config = BatchDaemonConfig {
        settings_refresh_interval: Duration::from_secs(30 * 60),
        settings_fetch_retry_interval: Duration::from_secs(20),
        idle_retry_interval: Duration::from_secs(20),
        token_refresh_threshold: Duration::from_secs(120),
        session_unavailable_log_interval: Duration::from_secs(5 * 60),
        continue_on_token_refresh_error: false,
    };

    let result = run_batch_daemon(
        &host,
        token_store,
        &auth_client,
        &api_client,
        &paths.batch_buffer_file,
        config,
    )
    .await;

    drop(power_monitor);
    result.map_err(Into::into)
}

fn update_daemon_status(
    paths: &ClientPaths,
    screenshot_permission: ScreenshotPermissionStatus,
    last_error: Option<String>,
) {
    let mut status = load_daemon_status(&paths.daemon_status_file).unwrap_or_default();
    status.screenshot_permission = screenshot_permission;
    status.last_error = last_error;
    status.updated_at = Some(Utc::now().to_rfc3339());
    let _ = save_daemon_status(&paths.daemon_status_file, &status);
}

fn is_permission_missing_error(error_text: &str) -> bool {
    let normalized = error_text.to_ascii_lowercase();
    normalized.contains("screen recording")
        || normalized.contains("not authorized")
        || normalized.contains("operation not permitted")
        || normalized.contains("permission denied")
}

fn detect_system_startup_event(paths: &ClientPaths) -> Option<DaemonAlertEvent> {
    let started_at = read_system_boot_time_utc()?;
    let boot_time_sec = started_at.timestamp();
    let boot_session_uuid = read_boot_session_uuid();

    let mut state = load_lifecycle_state(&paths.lifecycle_state_file);
    if state.last_seen_boot_time_sec == Some(boot_time_sec) {
        return None;
    }

    state.last_seen_boot_time_sec = Some(boot_time_sec);
    if let Some(uuid) = boot_session_uuid.clone() {
        state.last_seen_boot_session_uuid = Some(uuid);
    }
    if let Err(err) = save_lifecycle_state(&paths.lifecycle_state_file, &state) {
        eprintln!(
            "daemon: could not persist lifecycle state {}: {err}",
            paths.lifecycle_state_file.display()
        );
    }

    let mut metadata = vec![
        ("source".to_string(), "macos_kernel".to_string()),
        (
            "detected_by".to_string(),
            "kern_boottime_change".to_string(),
        ),
        ("boot_time_sec".to_string(), boot_time_sec.to_string()),
    ];
    if let Some(uuid) = boot_session_uuid {
        metadata.push(("boot_session_uuid".to_string(), uuid));
    }

    Some(DaemonAlertEvent {
        kind: "system_startup".to_string(),
        metadata,
        created_at: started_at,
        device_id: None,
    })
}

fn read_boot_session_uuid() -> Option<String> {
    run_sysctl_value("kern.bootsessionuuid")
}

fn read_system_boot_time_utc() -> Option<DateTime<Utc>> {
    let raw = run_sysctl_value("kern.boottime")?;
    let seconds = parse_boot_time_seconds(&raw)?;
    DateTime::<Utc>::from_timestamp(seconds, 0)
}

fn parse_boot_time_seconds(raw: &str) -> Option<i64> {
    let (_, rest) = raw.split_once("sec =")?;
    let trimmed = rest.trim_start();
    let digits: String = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<i64>().ok()
}

fn run_sysctl_value(key: &str) -> Option<String> {
    let output = Command::new("/usr/sbin/sysctl")
        .args(["-n", key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() { None } else { Some(value) }
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

#[cfg(test)]
mod tests {
    use super::parse_boot_time_seconds;

    #[test]
    fn parse_boot_time_seconds_extracts_sysctl_sec_value() {
        let raw = "{ sec = 1772592087, usec = 585398 } Tue Mar  3 21:41:27 2026";
        assert_eq!(parse_boot_time_seconds(raw), Some(1772592087));
    }
}
