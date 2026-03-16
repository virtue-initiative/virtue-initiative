use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use tokio::sync::mpsc;
use tokio::time::sleep;
use virtue_core::{LogEntry, MonitorService};

use crate::capture::{MacPlatformHooks, has_screen_capture_access, is_permission_missing_error};
use crate::config::{
    ClientPaths, DaemonStatus, ScreenshotPermissionStatus, build_core_config, save_daemon_status,
};

const ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(20);

pub async fn run_daemon(paths: &ClientPaths) -> Result<()> {
    paths.ensure_dirs()?;

    let shutdown = Arc::new(AtomicBool::new(false));
    let (signal_tx, mut signal_rx) = mpsc::unbounded_channel::<String>();
    spawn_signal_handler(shutdown.clone(), signal_tx);

    let mut service = MonitorService::setup(build_core_config(paths), MacPlatformHooks::new())?;
    emit_log(
        &mut service,
        "daemon_start",
        &[("source", "macos_launch_agent")],
    );

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
    let data = metadata
        .iter()
        .map(|(key, value)| {
            (
                (*key).to_string(),
                serde_json::Value::String((*value).to_string()),
            )
        })
        .collect::<serde_json::Map<_, _>>();

    let _ = service.send_log(LogEntry {
        ts_ms: Utc::now().timestamp_millis(),
        kind: kind.to_string(),
        risk: None,
        data: serde_json::Value::Object(data),
    });
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
