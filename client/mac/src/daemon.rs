use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::mpsc;
use virtue_core::{
    EventBus, FlushBatchNow, IpcBridge, LifecycleHooks, Ping, PlatformConfig, ProcessStarted,
    ProcessStopped, build_default_modules_reqwest, load_state, store_state,
};

use crate::capture::{MacPlatformHooks, has_screen_capture_access, is_permission_missing_error};
use crate::config::{ClientPaths, build_core_config};

const POST_WAKE_CAPTURE_STATE_SUPPRESSION: Duration = Duration::from_secs(30);
const ITER_INTERVAL: Duration = Duration::from_secs(1);
/// Boot-vs-monotonic divergence, measured locally each tick, worth treating as
/// "the machine just woke from sleep" for UX purposes (post-wake capture
/// suppression, a prompt batch flush). There's no real-time OS suspend/resume
/// notification anymore (the lifecycle model derives suspend retrospectively
/// from clocks rather than subscribing to sleep/wake events) — this constant
/// and the check built on it are independent daemon-loop UX plumbing, not
/// part of the core alerting model.
const LOCAL_SUSPEND_MIN_MS: i64 = 5_000;

/// Installs the process-wide `tracing` subscriber, writing daily-rotated
/// plain-text logs to `paths.logs_dir` (`~/Library/Logs/virtue.log`). The
/// returned guard must be kept alive for the life of the process — dropping
/// it stops the background writer thread that flushes buffered log lines.
///
/// A launchd stdout/stderr redirect (see `launch_agent.rs`) remains as a
/// fallback safety net for output emitted before this installs, or panics
/// that bypass `tracing` entirely.
pub fn init_logging(paths: &ClientPaths) -> tracing_appender::non_blocking::WorkerGuard {
    if let Err(err) = std::fs::create_dir_all(&paths.logs_dir) {
        eprintln!(
            "failed to create logs dir {}: {err}",
            paths.logs_dir.display()
        );
    }
    if let Err(err) = virtue_core::logging::prune_old_logs(
        &paths.logs_dir,
        &virtue_core::logging::DEFAULT_FILE_LOG_POLICY,
    ) {
        eprintln!("failed to prune old logs: {err}");
    }

    let file_appender = tracing_appender::rolling::daily(
        &paths.logs_dir,
        virtue_core::logging::DEFAULT_FILE_LOG_POLICY.file_name_prefix,
    );
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(virtue_core::logging::default_filter_directive(cfg!(
            debug_assertions
        )))
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();

    guard
}

pub fn run_daemon(paths: &ClientPaths) -> Result<()> {
    let paths = paths.clone();

    let result = tokio::runtime::Runtime::new()
        .map_err(anyhow::Error::from)
        .and_then(|runtime| runtime.block_on(run_daemon_service_loop(&paths)));
    if let Err(err) = result {
        tracing::error!(error = %format!("{err:#}"), "daemon: fatal error, exiting");
        std::process::exit(1);
    }
    std::process::exit(0);
}

async fn run_daemon_service_loop(paths: &ClientPaths) -> Result<()> {
    paths.ensure_dirs()?;

    let config = build_core_config(paths);
    let state_path = paths.state_dir.join("event_state.json");

    let platform = MacPlatformHooks::new();
    let modules = tokio::task::block_in_place(|| {
        build_default_modules_reqwest(config, platform.clone(), PlatformConfig::default())
    })?;
    let mut bus = EventBus::new(modules, load_state(&state_path)?)?;

    bus.send(ProcessStarted)?;
    store_state(&state_path, &bus.iter()?)?;

    let mut ipc = IpcBridge::bind(&paths.state_dir.join("daemon.sock"));
    if let Some(ipc) = &mut ipc {
        ipc.subscribe_standard_outbound(&mut bus);
    }

    let shutdown = Arc::new(AtomicBool::new(false));
    let (signal_tx, mut signal_rx) = mpsc::unbounded_channel::<String>();
    spawn_signal_handler(shutdown.clone(), signal_tx);

    let mut suppress_capture_state_until: Option<Instant> = None;
    let mut last_clocks: Option<(i64, i64)> = None;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        // Wire up any newly accepted IPC connections.
        if let Some(ipc) = &mut ipc {
            ipc.accept_pending(&mut bus, IpcBridge::forward_standard_inbound);
        }

        // Detect "just resumed from sleep" locally so post-wake capture
        // suppression and a prompt batch flush still happen, now that there's
        // no real-time OS suspend/resume subscription to trigger them.
        if let (Ok(boot_ms), Ok(mono_ms)) = (
            platform.get_boot_clock_ms(),
            platform.get_monotonic_clock_ms(),
        ) {
            if let Some((prev_boot_ms, prev_mono_ms)) = last_clocks {
                let suspend_ms = (boot_ms - prev_boot_ms) - (mono_ms - prev_mono_ms);
                if suspend_ms >= LOCAL_SUSPEND_MIN_MS {
                    suppress_capture_state_until =
                        Some(Instant::now() + POST_WAKE_CAPTURE_STATE_SUPPRESSION);
                    let _ = bus.send(FlushBatchNow);
                }
            }
            last_clocks = Some((boot_ms, mono_ms));
        }

        match tokio::task::block_in_place(|| {
            bus.send(Ping)?;
            bus.iter()
        }) {
            Ok(state) => {
                if !has_screen_capture_access() {
                    suppress_capture_state_until = None;
                }
                if let Err(e) = store_state(&state_path, &state) {
                    tracing::error!(error = %e, "daemon: failed to store state");
                }
            }
            Err(err) => {
                let error_text = err.to_string();
                if suppress_capture_state_until.is_none_or(|until| Instant::now() >= until) {
                    if is_permission_missing_error(&error_text) {
                        tracing::warn!("daemon: capture permission missing: {error_text}");
                    } else {
                        tracing::warn!("daemon: capture blocked: {error_text}");
                    }
                }
                tracing::warn!("daemon: {error_text}");
            }
        }

        tokio::select! {
            signal = signal_rx.recv() => {
                if signal.is_some() {
                    tokio::task::block_in_place(|| {
                        let _ = bus.send(ProcessStopped);
                        if let Ok(state) = bus.iter() {
                            let _ = store_state(&state_path, &state);
                        }
                    });
                }
                break;
            }
            _ = tokio::time::sleep(ITER_INTERVAL) => {}
        }
    }

    tokio::task::block_in_place(|| {
        let _ = bus.send(Ping);
        if let Ok(state) = bus.iter() {
            let _ = store_state(&state_path, &state);
        }
    });
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
