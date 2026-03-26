use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::sync::mpsc;
use tokio::time::sleep;
use virtue_core::{LogEntry, MonitorService};

use crate::capture::{LinuxPlatformHooks, is_session_unavailable_text};
use crate::config::{ClientPaths, build_core_config};
use crate::tray;

const CURRENT_BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";
const PROC_STAT_PATH: &str = "/proc/stat";
const SESSION_UNAVAILABLE_LOG_INTERVAL: Duration = Duration::from_secs(5 * 60);
const ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(20);

#[derive(Debug, Default, Serialize, Deserialize)]
struct LifecycleState {
    last_seen_boot_id: Option<String>,
}

pub async fn run_daemon(paths: &ClientPaths) -> Result<()> {
    paths.ensure_dirs()?;
    let _tray = tray::start_daemon_tray(paths.clone());

    let shutdown = Arc::new(AtomicBool::new(false));
    let mut service = MonitorService::setup(build_core_config(paths), LinuxPlatformHooks::new())?;

    emit_log(
        &mut service,
        "daemon_start",
        &[("source", "linux_service")],
        Utc::now(),
    );
    if let Some(startup_event) = detect_system_startup_event(paths) {
        emit_log_entry(&mut service, startup_event);
    }

    let (signal_tx, mut signal_rx) = mpsc::unbounded_channel::<String>();
    spawn_signal_handler(shutdown.clone(), signal_tx);

    let mut last_session_unavailable_log: Option<Instant> = None;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        let sleep_duration = match service.loop_iteration() {
            Ok(outcome) => {
                last_session_unavailable_log = None;
                duration_until(outcome.next_run_at_ms)
            }
            Err(err) => {
                let message = err.to_string();
                if is_session_unavailable_text(&message) {
                    let should_log = last_session_unavailable_log
                        .is_none_or(|last| last.elapsed() >= SESSION_UNAVAILABLE_LOG_INTERVAL);
                    if should_log {
                        eprintln!("daemon: capture session unavailable: {message}");
                        last_session_unavailable_log = Some(Instant::now());
                    }
                } else {
                    eprintln!("daemon: {message}");
                }
                ERROR_RETRY_INTERVAL
            }
        };

        tokio::select! {
            signal = signal_rx.recv() => {
                if let Some(signal_name) = signal {
                    emit_shutdown_logs(&mut service, &signal_name);
                }
                break;
            }
            _ = sleep_interruptible(&shutdown, sleep_duration) => {}
        }
    }

    let _ = service.shutdown();
    Ok(())
}

fn detect_system_startup_event(paths: &ClientPaths) -> Option<LogEntry> {
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
    Some(log_entry(
        "system_startup",
        &[
            ("source", "linux_system"),
            ("boot_id", boot_id.as_str()),
            ("detected_by", "boot_id_change"),
        ],
        started_at,
    ))
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

fn spawn_signal_handler(shutdown: Arc<AtomicBool>, signal_tx: mpsc::UnboundedSender<String>) {
    tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(_) => return,
        };
        let mut sigint = signal(SignalKind::interrupt()).ok();

        let signal_name = tokio::select! {
            _ = sigterm.recv() => "SIGTERM",
            _ = async {
                match sigint.as_mut() {
                    Some(signal) => signal.recv().await,
                    None => std::future::pending::<Option<()>>().await,
                }
            } => "SIGINT",
        };

        shutdown.store(true, Ordering::SeqCst);
        let _ = signal_tx.send(signal_name.to_string());
    });
}

async fn sleep_interruptible(shutdown: &Arc<AtomicBool>, duration: Duration) {
    let mut remaining = duration;
    while remaining > Duration::ZERO && !shutdown.load(Ordering::SeqCst) {
        let tick = remaining.min(Duration::from_secs(1));
        sleep(tick).await;
        remaining = remaining.saturating_sub(tick);
    }
}

fn duration_until(next_run_at_ms: i64) -> Duration {
    let now_ms = Utc::now().timestamp_millis();
    let delta_ms = next_run_at_ms.saturating_sub(now_ms);
    Duration::from_millis(delta_ms.max(0) as u64)
}

fn emit_shutdown_logs(service: &mut MonitorService<LinuxPlatformHooks>, signal_name: &str) {
    emit_log(
        service,
        "daemon_stop_signal",
        &[("signal", signal_name)],
        Utc::now(),
    );

    let system_state = read_systemd_state();
    let shutting_down =
        matches!(system_state.as_deref(), Some("stopping")) || is_shutdown_job_queued();
    if shutting_down {
        let mut metadata = vec![
            ("source".to_string(), "linux_system".to_string()),
            ("signal".to_string(), signal_name.to_string()),
        ];
        if let Some(system_state) = system_state {
            metadata.push(("system_state".to_string(), system_state));
        }
        emit_log_entry(
            service,
            LogEntry {
                ts_ms: Utc::now().timestamp_millis(),
                kind: "system_shutdown".to_string(),
                risk: None,
                data: metadata_value_owned(metadata),
            },
        );
    }
}

fn emit_log(
    service: &mut MonitorService<LinuxPlatformHooks>,
    kind: &str,
    metadata: &[(&str, &str)],
    created_at: DateTime<Utc>,
) {
    emit_log_entry(service, log_entry(kind, metadata, created_at));
}

fn emit_log_entry(service: &mut MonitorService<LinuxPlatformHooks>, entry: LogEntry) {
    if let Err(err) = service.send_log(entry) {
        eprintln!("daemon: could not send log event: {err}");
    }
}

fn log_entry(kind: &str, metadata: &[(&str, &str)], created_at: DateTime<Utc>) -> LogEntry {
    LogEntry {
        ts_ms: created_at.timestamp_millis(),
        kind: kind.to_string(),
        risk: None,
        data: metadata_value(metadata),
    }
}

fn metadata_value(metadata: &[(&str, &str)]) -> Value {
    let mut object = Map::new();
    for (key, value) in metadata {
        object.insert((*key).to_string(), Value::String((*value).to_string()));
    }
    Value::Object(object)
}

fn metadata_value_owned(metadata: Vec<(String, String)>) -> Value {
    let mut object = Map::new();
    for (key, value) in metadata {
        object.insert(key, Value::String(value));
    }
    Value::Object(object)
}
