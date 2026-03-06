use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use virtue_client_core::{
    ApiClient, AuthClient, BatchDaemonConfig, CaptureOutcome, CoreError, DaemonAlertEvent,
    FileTokenStore, PersistedServiceState, ServiceEvent, ServiceHost, SleepOutcome, TokenStore,
    run_batch_daemon,
};

use crate::capture::{capture_screen, is_session_unavailable_error};
use crate::config::{ClientPaths, load_state};
use crate::tray;

const CURRENT_BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";
const PROC_STAT_PATH: &str = "/proc/stat";

#[derive(Debug, Default, Serialize, Deserialize)]
struct LifecycleState {
    last_seen_boot_id: Option<String>,
}

#[derive(Clone)]
struct LinuxDaemonHost {
    paths: ClientPaths,
    shutdown: Arc<AtomicBool>,
    pending_alert_events: Arc<Mutex<Vec<DaemonAlertEvent>>>,
}

impl LinuxDaemonHost {
    fn new(
        paths: ClientPaths,
        shutdown: Arc<AtomicBool>,
        pending_alert_events: Arc<Mutex<Vec<DaemonAlertEvent>>>,
    ) -> Self {
        Self {
            paths,
            shutdown,
            pending_alert_events,
        }
    }
}

impl ServiceHost for LinuxDaemonHost {
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
        let state =
            load_state(&self.paths.state_file).map_err(|e| CoreError::Platform(e.to_string()))?;
        match capture_screen(state.backend_hint) {
            Ok(bytes) => Ok(CaptureOutcome::FramePng(bytes)),
            Err(err) if is_session_unavailable_error(&err) => {
                Ok(CaptureOutcome::SessionUnavailable)
            }
            Err(err) => Err(CoreError::Platform(err.to_string())),
        }
    }

    fn emit_event(&self, event: ServiceEvent) {
        match event {
            ServiceEvent::Info(msg) => eprintln!("daemon: {msg}"),
            ServiceEvent::Warn(msg) => eprintln!("daemon: {msg}"),
            ServiceEvent::Error(msg) => eprintln!("daemon: {msg}"),
        }
    }

    fn should_stop(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    fn drain_alert_events(&self) -> virtue_client_core::CoreResult<Vec<DaemonAlertEvent>> {
        let mut guard = self
            .pending_alert_events
            .lock()
            .map_err(|_| CoreError::Platform("alert event queue lock poisoned".to_string()))?;
        Ok(std::mem::take(&mut *guard))
    }
}

pub async fn run_daemon(paths: &ClientPaths) -> Result<()> {
    paths.ensure_dirs()?;
    let _tray = tray::start_daemon_tray(paths.clone());

    let shutdown = Arc::new(AtomicBool::new(false));
    let mut startup_events = vec![DaemonAlertEvent {
        kind: "daemon_start".to_string(),
        metadata: vec![("source".to_string(), "linux_service".to_string())],
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

            if let Ok(mut guard) = pending_alert_events.lock() {
                guard.push(DaemonAlertEvent {
                    kind: "daemon_stop_signal".to_string(),
                    metadata: vec![("signal".to_string(), signal_name.to_string())],
                    created_at: Utc::now(),
                    device_id: None,
                });

                let system_state = read_systemd_state();
                let shutting_down =
                    matches!(system_state.as_deref(), Some("stopping")) || is_shutdown_job_queued();
                if shutting_down {
                    let mut metadata = vec![("source".to_string(), "linux_system".to_string())];
                    metadata.push(("signal".to_string(), signal_name.to_string()));
                    if let Some(state) = system_state {
                        metadata.push(("system_state".to_string(), state));
                    }
                    guard.push(DaemonAlertEvent {
                        kind: "system_shutdown".to_string(),
                        metadata,
                        created_at: Utc::now(),
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
    let host = LinuxDaemonHost::new(paths.clone(), shutdown, pending_alert_events);

    let config = BatchDaemonConfig {
        settings_refresh_interval: Duration::from_secs(30 * 60),
        settings_fetch_retry_interval: Duration::from_secs(20),
        idle_retry_interval: Duration::from_secs(20),
        token_refresh_threshold: Duration::from_secs(120),
        session_unavailable_log_interval: Duration::from_secs(5 * 60),
        continue_on_token_refresh_error: false,
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

fn detect_system_startup_event(paths: &ClientPaths) -> Option<DaemonAlertEvent> {
    let boot_id = read_current_boot_id()?;
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

    let started_at = read_system_boot_time_utc().unwrap_or_else(Utc::now);
    Some(DaemonAlertEvent {
        kind: "system_startup".to_string(),
        metadata: vec![
            ("source".to_string(), "linux_system".to_string()),
            ("boot_id".to_string(), boot_id),
            ("detected_by".to_string(), "boot_id_change".to_string()),
        ],
        created_at: started_at,
        device_id: None,
    })
}

fn read_current_boot_id() -> Option<String> {
    fs::read_to_string(CURRENT_BOOT_ID_PATH)
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn read_system_boot_time_utc() -> Option<DateTime<Utc>> {
    let raw = fs::read_to_string(PROC_STAT_PATH).ok()?;
    let seconds = raw
        .lines()
        .find_map(|line| line.strip_prefix("btime "))
        .and_then(|value| value.trim().parse::<i64>().ok())?;
    DateTime::<Utc>::from_timestamp(seconds, 0)
}

fn read_systemd_state() -> Option<String> {
    let output = Command::new("systemctl")
        .arg("is-system-running")
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_ascii_lowercase();
    if !stdout.is_empty() {
        return Some(stdout);
    }

    let stderr = String::from_utf8_lossy(&output.stderr)
        .trim()
        .to_ascii_lowercase();
    if !stderr.is_empty() {
        return Some(stderr);
    }
    None
}

fn is_shutdown_job_queued() -> bool {
    let output = match Command::new("systemctl")
        .args(["list-jobs", "--no-legend", "--no-pager"])
        .output()
    {
        Ok(value) => value,
        Err(_) => return false,
    };

    if !output.status.success() {
        return false;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    stdout.lines().any(|line| {
        line.contains("shutdown.target") && (line.contains(" start ") || line.ends_with(" start"))
    })
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
