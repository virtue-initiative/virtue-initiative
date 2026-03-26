use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use block2::RcBlock;
use chrono::{DateTime, Utc};
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSWorkspace, NSWorkspaceWillPowerOffNotification};
use objc2_foundation::{NSDate, NSRunLoop};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::sleep;
use virtue_core::{LogEntry, MonitorService};

use crate::capture::{MacPlatformHooks, has_screen_capture_access, is_permission_missing_error};
use crate::config::{
    ClientPaths, DaemonStatus, ScreenshotPermissionStatus, build_core_config, save_daemon_status,
};

const ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(20);

#[derive(Debug, Default, Serialize, Deserialize)]
struct LifecycleState {
    last_seen_boot_id: Option<String>,
}

struct ShutdownWatcher {
    should_stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ShutdownWatcher {
    fn spawn(shutdown_requested: Arc<AtomicBool>) -> Self {
        let should_stop = Arc::new(AtomicBool::new(false));
        let worker_stop = should_stop.clone();
        let worker = thread::spawn(move || {
            autoreleasepool(|_| unsafe {
                let workspace = NSWorkspace::sharedWorkspace();
                let center = workspace.notificationCenter();
                let callback = RcBlock::new(move |_| {
                    shutdown_requested.store(true, Ordering::SeqCst);
                });
                let observer = center.addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceWillPowerOffNotification),
                    None,
                    None,
                    &callback,
                );
                let run_loop = NSRunLoop::currentRunLoop();

                while !worker_stop.load(Ordering::SeqCst) {
                    let next_tick = NSDate::dateWithTimeIntervalSinceNow(0.5);
                    run_loop.runUntilDate(&next_tick);
                }

                center.removeObserver((*observer).as_ref());
            });
        });

        Self {
            should_stop,
            worker: Some(worker),
        }
    }
}

impl Drop for ShutdownWatcher {
    fn drop(&mut self) {
        self.should_stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub async fn run_daemon(paths: &ClientPaths) -> Result<()> {
    paths.ensure_dirs()?;

    let shutdown = Arc::new(AtomicBool::new(false));
    let system_shutdown_requested = Arc::new(AtomicBool::new(false));
    let (signal_tx, mut signal_rx) = mpsc::unbounded_channel::<String>();
    spawn_signal_handler(shutdown.clone(), signal_tx);
    let _shutdown_watcher = ShutdownWatcher::spawn(system_shutdown_requested.clone());

    let mut service = MonitorService::setup(build_core_config(paths), MacPlatformHooks::new())?;
    emit_log(
        &mut service,
        "daemon_start",
        &[("source", "macos_launch_agent")],
    );
    if let Some(startup_event) = detect_system_startup_event(paths) {
        emit_log_entry(&mut service, startup_event);
    }

    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        let sleep_duration = match service.loop_iteration() {
            Ok(outcome) => {
                update_daemon_status(paths, current_permission_status(), None);
                duration_until(outcome.next_run_at_ms)
            }
            Err(err) => {
                let error_text = err.to_string();
                let permission = if is_permission_missing_error(&error_text) {
                    ScreenshotPermissionStatus::Missing
                } else {
                    current_permission_status()
                };
                update_daemon_status(paths, permission, Some(error_text.clone()));
                eprintln!("daemon: {error_text}");
                ERROR_RETRY_INTERVAL
            }
        };

        tokio::select! {
            signal = signal_rx.recv() => {
                if let Some(signal_name) = signal {
                    emit_log(
                        &mut service,
                        "daemon_stop_signal",
                        &[("signal", signal_name.as_str())],
                    );
                    if system_shutdown_requested.load(Ordering::SeqCst) {
                        emit_log(
                            &mut service,
                            "system_shutdown",
                            &[
                                ("source", "macos_system"),
                                ("signal", signal_name.as_str()),
                                ("detected_by", "nsworkspace_will_power_off"),
                            ],
                        );
                    }
                }
                break;
            }
            _ = sleep_interruptible(&shutdown, sleep_duration) => {}
        }
    }

    let _ = service.shutdown();
    update_daemon_status(paths, current_permission_status(), None);
    Ok(())
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

    Some(log_entry(
        "system_startup",
        &[
            ("source", "macos_system"),
            ("boot_id", boot_id.as_str()),
            ("detected_by", "kern_boottime_change"),
        ],
        started_at,
    ))
}

fn current_permission_status() -> ScreenshotPermissionStatus {
    if has_screen_capture_access() {
        ScreenshotPermissionStatus::Granted
    } else {
        ScreenshotPermissionStatus::Missing
    }
}

fn update_daemon_status(
    paths: &ClientPaths,
    screenshot_permission: ScreenshotPermissionStatus,
    last_error: Option<String>,
) {
    let status = DaemonStatus {
        screenshot_permission,
        last_error,
        updated_at: Some(Utc::now().to_rfc3339()),
    };
    if let Err(err) = save_daemon_status(&paths.daemon_status_file, &status) {
        eprintln!(
            "daemon: failed to write status {}: {err}",
            paths.daemon_status_file.display()
        );
    }
}

fn emit_log(service: &mut MonitorService<MacPlatformHooks>, kind: &str, metadata: &[(&str, &str)]) {
    emit_log_entry(service, log_entry(kind, metadata, Utc::now()));
}

fn emit_log_entry(service: &mut MonitorService<MacPlatformHooks>, entry: LogEntry) {
    let _ = service.send_log(entry);
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

fn current_boot_marker() -> Option<(String, DateTime<Utc>)> {
    let output = Command::new("/usr/sbin/sysctl")
        .args(["-n", "kern.boottime"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let seconds = parse_sysctl_component(&stdout, "sec = ")?;
    let usec = parse_sysctl_component(&stdout, "usec = ")?;
    let started_at = DateTime::<Utc>::from_timestamp(seconds, (usec as u32).saturating_mul(1_000))?;
    let boot_id = format!("{seconds}:{usec}");
    Some((boot_id, started_at))
}

fn parse_sysctl_component(text: &str, prefix: &str) -> Option<i64> {
    let start = text.find(prefix)? + prefix.len();
    let rest = &text[start..];
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok()
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

fn log_entry(kind: &str, metadata: &[(&str, &str)], ts: DateTime<Utc>) -> LogEntry {
    let data = metadata
        .iter()
        .map(|(key, value)| {
            (
                (*key).to_string(),
                serde_json::Value::String((*value).to_string()),
            )
        })
        .collect::<serde_json::Map<_, _>>();

    LogEntry {
        ts_ms: ts.timestamp_millis(),
        kind: kind.to_string(),
        risk: None,
        data: serde_json::Value::Object(data),
    }
}
