use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use tokio::sync::mpsc;
use virtue_core::ProcessStoppedReason;
use virtue_core::{
    EventBus, EventChannel, IpcBridge, Ping, PlatformConfig, ProcessStarted, ProcessStopped,
    SystemLogoutObserved, UserStopRequested, build_default_modules_reqwest, load_state,
    store_state,
};

use crate::capture::{LinuxPlatformHooks, is_session_unavailable_text};
use crate::config::{ClientPaths, build_core_config};
use crate::tray;

const SESSION_UNAVAILABLE_LOG_INTERVAL: Duration = Duration::from_secs(5 * 60);
const ITER_INTERVAL: Duration = Duration::from_secs(1);

pub async fn run_daemon(paths: &ClientPaths) -> Result<()> {
    paths.ensure_dirs()?;
    let _tray = tray::start_daemon_tray(paths.clone());

    let config = build_core_config(paths);
    let state_path = paths.state_dir.join("event_state.json");
    let modules = tokio::task::block_in_place(|| {
        build_default_modules_reqwest(config, LinuxPlatformHooks::new(), PlatformConfig::default())
    })?;
    let mut bus = EventBus::new(modules, load_state(&state_path)?)?;

    tokio::task::block_in_place(|| {
        bus.send(ProcessStarted)?;
        store_state(&state_path, &bus.iter()?)
    })?;

    let mut ipc = IpcBridge::bind(&paths.state_dir.join("daemon.sock"));
    if let Some(ipc) = &mut ipc {
        ipc.subscribe_standard_outbound(&mut bus);
    }

    let shutdown = Arc::new(AtomicBool::new(false));
    let user_stop_requested = Arc::new(AtomicBool::new(false));
    let (signal_tx, mut signal_rx) = mpsc::unbounded_channel::<String>();
    spawn_signal_handler(shutdown.clone(), signal_tx);

    let mut last_session_unavailable_log: Option<std::time::Instant> = None;
    let mut shutdown_cleanup_done = false;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        // Wire up any newly accepted IPC connections.
        if let Some(ipc) = &mut ipc {
            let usr = user_stop_requested.clone();
            ipc.accept_pending(&mut bus, move |remote, e| {
                IpcBridge::forward_standard_inbound(remote, e);
                // Track user-stop separately to classify shutdown reason accurately.
                let usr = usr.clone();
                remote.on::<UserStopRequested>(move |_ev| {
                    usr.store(true, Ordering::SeqCst);
                    Ok(())
                });
            });
        }

        match tokio::task::block_in_place(|| {
            bus.send(Ping)?;
            bus.iter()
        }) {
            Ok(state) => {
                last_session_unavailable_log = None;
                if let Err(e) = store_state(&state_path, &state) {
                    eprintln!("daemon: failed to store state: {e}");
                }
            }
            Err(err) => {
                let message = err.to_string();
                if is_session_unavailable_text(&message) {
                    let should_log = last_session_unavailable_log
                        .is_none_or(|last| last.elapsed() >= SESSION_UNAVAILABLE_LOG_INTERVAL);
                    if should_log {
                        eprintln!("daemon: capture session unavailable: {message}");
                        last_session_unavailable_log = Some(std::time::Instant::now());
                    }
                } else {
                    eprintln!("daemon: {message}");
                }
            }
        }

        tokio::select! {
            signal = signal_rx.recv() => {
                if signal.is_some() {
                    let explicit_user_stop = user_stop_requested.load(Ordering::SeqCst);
                    tokio::task::block_in_place(|| {
                        record_shutdown_transition(&mut bus, &state_path, explicit_user_stop)
                    });
                    shutdown_cleanup_done = true;
                }
                break;
            }
            _ = tokio::time::sleep(ITER_INTERVAL) => {}
        }
    }

    if !shutdown_cleanup_done {
        tokio::task::block_in_place(|| {
            let _ = bus.send(Ping);
            if let Ok(state) = bus.iter() {
                let _ = store_state(&state_path, &state);
            }
        });
    }
    std::process::exit(0);
}

const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(2);

fn run_with_timeout<F, T>(f: F) -> Option<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(SYSTEMCTL_TIMEOUT).ok()
}

fn read_systemd_state() -> Option<String> {
    run_with_timeout(|| {
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
            Some(stderr)
        } else {
            None
        }
    })
    .flatten()
}

fn is_shutdown_job_queued() -> bool {
    run_with_timeout(|| {
        let output = Command::new("systemctl")
            .args(["list-jobs", "--no-legend", "--no-pager"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
        Some(stdout.lines().any(|line| {
            line.contains("shutdown.target")
                && (line.contains(" start ") || line.ends_with(" start"))
        }))
    })
    .flatten()
    .unwrap_or(false)
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

fn classify_shutdown_reason(
    system_state: Option<&str>,
    shutdown_job_queued: bool,
    explicit_user_stop: bool,
) -> ProcessStoppedReason {
    let shutting_down = matches!(system_state, Some("stopping")) || shutdown_job_queued;
    if shutting_down {
        ProcessStoppedReason::Shutdown
    } else if explicit_user_stop {
        ProcessStoppedReason::User
    } else {
        ProcessStoppedReason::Other
    }
}

fn record_shutdown_transition(
    bus: &mut EventBus,
    state_path: &std::path::Path,
    explicit_user_stop: bool,
) {
    let system_state = read_systemd_state();
    let shutdown_job_queued = is_shutdown_job_queued();
    let reason = classify_shutdown_reason(
        system_state.as_deref(),
        shutdown_job_queued,
        explicit_user_stop,
    );
    // Only a genuine system shutdown gives us an exact logout moment — an
    // `Other` stop (crash, `kill`, `systemctl stop`) doesn't tell us the
    // session actually ended, so it shouldn't claim one.
    if matches!(reason, ProcessStoppedReason::Shutdown) {
        let utc_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let _ = bus.send(SystemLogoutObserved { utc_ms });
    }
    let _ = bus.send(ProcessStopped(reason));
    let _ = bus.send(Ping);
    if let Ok(state) = bus.iter() {
        let _ = store_state(state_path, &state);
    }
}

#[cfg(test)]
mod tests {
    use virtue_core::ProcessStoppedReason;

    use super::classify_shutdown_reason;

    #[test]
    fn classify_shutdown_reason_produces_shutdown_on_systemd_stopping_state() {
        assert!(matches!(
            classify_shutdown_reason(Some("stopping"), false, false),
            ProcessStoppedReason::Shutdown
        ));
    }

    #[test]
    fn classify_shutdown_reason_produces_shutdown_when_job_queued() {
        assert!(matches!(
            classify_shutdown_reason(None, true, false),
            ProcessStoppedReason::Shutdown
        ));
    }

    #[test]
    fn classify_shutdown_reason_produces_user_on_user_stop() {
        assert!(matches!(
            classify_shutdown_reason(None, false, true),
            ProcessStoppedReason::User
        ));
    }

    #[test]
    fn classify_shutdown_reason_produces_other_by_default() {
        assert!(matches!(
            classify_shutdown_reason(None, false, false),
            ProcessStoppedReason::Other
        ));
    }

    #[test]
    fn classify_shutdown_reason_shutdown_takes_priority_over_user_stop() {
        assert!(matches!(
            classify_shutdown_reason(Some("stopping"), false, true),
            ProcessStoppedReason::Shutdown
        ));
    }
}
