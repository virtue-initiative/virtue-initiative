use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::mpsc;
use virtue_core::events::{Event, ProcessStoppedReason};
use virtue_core::{MonitorService, iter_sleep};
use zbus::proxy;

use crate::capture::{LinuxPlatformHooks, is_session_unavailable_text};
use crate::config::{ClientPaths, build_core_config};
use crate::tray;

const SESSION_UNAVAILABLE_LOG_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait LoginManager {
    #[zbus(signal)]
    fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;
}

pub async fn run_daemon(paths: &ClientPaths) -> Result<()> {
    paths.ensure_dirs()?;
    let _tray = tray::start_daemon_tray(paths.clone());

    let shutdown = Arc::new(AtomicBool::new(false));
    let mut service = MonitorService::setup(build_core_config(paths), LinuxPlatformHooks::new())?;

    service.queue_event(Event::ProcessStarted);
    let _ = service.run_event_loop_iter();

    let (signal_tx, mut signal_rx) = mpsc::unbounded_channel::<String>();
    spawn_signal_handler(shutdown.clone(), signal_tx);
    let (sleep_tx, mut sleep_rx) = mpsc::unbounded_channel::<bool>();
    spawn_suspend_watcher(sleep_tx);

    let mut last_session_unavailable_log: Option<Instant> = None;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        match service.loop_iteration() {
            Ok(_outcome) => {
                last_session_unavailable_log = None;
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
            }
        }

        tokio::select! {
            signal = signal_rx.recv() => {
                if signal.is_some() {
                    record_shutdown_transition(&mut service);
                }
                break;
            }
            sleep_change = sleep_rx.recv() => {
                if let Some(suspending) = sleep_change {
                    let event = if suspending {
                        Event::ComputerSuspended
                    } else {
                        Event::ComputerResumed
                    };
                    service.queue_event(event);
                    let _ = service.run_event_loop_iter();
                }
            }
            _ = iter_sleep() => {}
        }
    }

    let _ = service.run_event_loop_iter();
    let _ = service.mark_stopped();
    Ok(())
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

fn spawn_suspend_watcher(sleep_tx: mpsc::UnboundedSender<bool>) {
    use futures_util::StreamExt;
    tokio::spawn(async move {
        let connection = match zbus::Connection::system().await {
            Ok(connection) => connection,
            Err(err) => {
                eprintln!("daemon: failed connecting to system bus for suspend watcher: {err}");
                return;
            }
        };

        let proxy = match LoginManagerProxy::new(&connection).await {
            Ok(proxy) => proxy,
            Err(err) => {
                eprintln!("daemon: failed creating login1 proxy for suspend watcher: {err}");
                return;
            }
        };

        let mut stream = match proxy.receive_prepare_for_sleep().await {
            Ok(stream) => stream,
            Err(err) => {
                eprintln!("daemon: failed subscribing to login1 PrepareForSleep: {err}");
                return;
            }
        };

        while let Some(signal) = stream.next().await {
            match signal.args() {
                Ok(args) => {
                    let _ = sleep_tx.send(*args.start());
                }
                Err(err) => {
                    eprintln!("daemon: failed decoding login1 PrepareForSleep signal: {err}");
                }
            }
        }
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

fn record_shutdown_transition(service: &mut MonitorService<LinuxPlatformHooks>) {
    let system_state = read_systemd_state();
    let shutdown_job_queued = is_shutdown_job_queued();
    let explicit_user_stop = service.take_stop_intent().ok().flatten().is_some();
    let reason = classify_shutdown_reason(
        system_state.as_deref(),
        shutdown_job_queued,
        explicit_user_stop,
    );
    service.queue_event(Event::ProcessStopped(reason));
    let _ = service.run_event_loop_iter();
}

#[cfg(test)]
mod tests {
    use virtue_core::events::ProcessStoppedReason;

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
