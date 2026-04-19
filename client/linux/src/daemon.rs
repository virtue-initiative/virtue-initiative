use std::fs;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use chrono::Utc;
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio::time::sleep;
use virtue_core::{
    CaptureAvailabilityState, ComputerPowerState, LifecycleConfidence, LifecycleObservation,
    LifecycleOrigin, MonitorService, ServiceRole,
};
use zbus::proxy;

use crate::capture::{LinuxPlatformHooks, is_session_unavailable_text};
use crate::config::{ClientPaths, build_core_config};
use crate::tray;

const CURRENT_BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";
const PROC_STAT_PATH: &str = "/proc/stat";
const SERVICE_PING_WAKE_PADDING_MS: i64 = 1_000;
const SESSION_UNAVAILABLE_LOG_INTERVAL: Duration = Duration::from_secs(5 * 60);
const ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(20);

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
    if let Some(boot_marker) = read_current_boot_id() {
        let _ = service.record_lifecycle_observation(LifecycleObservation::BootObserved {
            boot_marker,
            booted_at_ms: read_current_boot_time_ms(),
            detected_by: "boot_id_change".to_string(),
        });
    }
    let _ = service.record_lifecycle_observation(LifecycleObservation::ServiceStarted {
        role: ServiceRole::PrimaryService,
        detected_by: "linux_service".to_string(),
    });

    let (signal_tx, mut signal_rx) = mpsc::unbounded_channel::<String>();
    spawn_signal_handler(shutdown.clone(), signal_tx);
    let (sleep_tx, mut sleep_rx) = mpsc::unbounded_channel::<bool>();
    spawn_suspend_watcher(sleep_tx);

    let mut last_session_unavailable_log: Option<Instant> = None;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        let sleep_duration = match service.loop_iteration() {
            Ok(outcome) => {
                let _ = service.record_lifecycle_observation(
                    LifecycleObservation::CaptureAvailabilityChanged {
                        state: CaptureAvailabilityState::Ready,
                        detected_by: "successful_loop".to_string(),
                    },
                );
                last_session_unavailable_log = None;
                duration_until(outcome.next_run_at_ms)
            }
            Err(err) => {
                let message = err.to_string();
                if is_session_unavailable_text(&message) {
                    let _ = service.record_lifecycle_observation(
                        LifecycleObservation::CaptureAvailabilityChanged {
                            state: CaptureAvailabilityState::Blocked,
                            detected_by: "session_unavailable".to_string(),
                        },
                    );
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
        let _ =
            service.record_service_ping_if_due(ServiceRole::PrimaryService, "linux_service_timer");
        let sleep_duration = service
            .next_service_ping_due_at_ms(ServiceRole::PrimaryService)
            .ok()
            .flatten()
            .map(|due_at_ms| duration_until(due_at_ms.saturating_add(SERVICE_PING_WAKE_PADDING_MS)))
            .map(|ping_duration| ping_duration.min(sleep_duration))
            .unwrap_or(sleep_duration);

        tokio::select! {
            signal = signal_rx.recv() => {
                if let Some(signal_name) = signal {
                    record_shutdown_transition(&mut service, &signal_name);
                }
                break;
            }
            sleep_change = sleep_rx.recv() => {
                if let Some(suspending) = sleep_change {
                    let (state, origin, confidence) = if suspending {
                        (
                            ComputerPowerState::Suspended,
                            LifecycleOrigin::SystemSuspend,
                            LifecycleConfidence::Confirmed,
                        )
                    } else {
                        (
                            ComputerPowerState::Running,
                            LifecycleOrigin::SystemSuspend,
                            LifecycleConfidence::Confirmed,
                        )
                    };
                    let _ = service.record_lifecycle_observation(
                        LifecycleObservation::ComputerPowerChanged {
                            state,
                            origin,
                            detected_by: "login1_prepare_for_sleep".to_string(),
                            confidence,
                        },
                    );
                }
            }
            _ = sleep_interruptible(&shutdown, sleep_duration) => {}
        }
    }

    let _ = service.shutdown();
    Ok(())
}

fn read_current_boot_id() -> Option<String> {
    fs::read_to_string(CURRENT_BOOT_ID_PATH)
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn read_current_boot_time_ms() -> Option<i64> {
    let stat = fs::read_to_string(PROC_STAT_PATH).ok()?;
    let boot_line = stat.lines().find(|line| line.starts_with("btime "))?;
    let seconds = boot_line.split_whitespace().nth(1)?.parse::<i64>().ok()?;
    seconds.checked_mul(1_000)
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

fn record_shutdown_transition(service: &mut MonitorService<LinuxPlatformHooks>, signal_name: &str) {
    let system_state = read_systemd_state();
    let shutting_down =
        matches!(system_state.as_deref(), Some("stopping")) || is_shutdown_job_queued();
    let explicit_user_stop = service
        .take_stop_intent(ServiceRole::PrimaryService)
        .ok()
        .flatten()
        .is_some();
    let _ = service.record_lifecycle_observation(LifecycleObservation::ServiceStopObserved {
        role: ServiceRole::PrimaryService,
        raw_reason: signal_name.to_string(),
        shutdown_in_progress: shutting_down,
        explicit_user_stop,
        detected_by: "signal_plus_system_state".to_string(),
    });
}
