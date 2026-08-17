use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use tokio::sync::mpsc;
use virtue_core::api::ReqwestApiClient;
use virtue_core::{Daemon, IpcBridge};

use crate::capture::MacPlatformHooks;
use crate::config::{ClientPaths, build_core_config};

const IPC_ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// Boot-vs-monotonic divergence, measured locally each poll, worth treating as
/// "the machine just woke from sleep" for UX purposes (a prompt batch flush).
/// There's no real-time OS suspend/resume notification anymore (the core
/// lifecycle model no longer tracks suspend at all) — this constant and the
/// check built on it are independent daemon-loop UX plumbing, not part of the
/// core alerting model. See `client/CLAUDE.md`.
const LOCAL_SUSPEND_MIN_MS: i64 = 5_000;

type MacDaemon = Daemon<MacPlatformHooks, ReqwestApiClient>;

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

    let daemon: Arc<MacDaemon> = Arc::new(tokio::task::block_in_place(|| {
        let api = ReqwestApiClient::new(&config)?;
        Daemon::new(config, platform.clone(), api, state_path)
    })?);

    let mut ipc = IpcBridge::bind(&paths.state_dir.join("daemon.sock"));

    let shutdown = Arc::new(AtomicBool::new(false));
    let (signal_tx, mut signal_rx) = mpsc::unbounded_channel::<String>();
    spawn_signal_handler(Arc::clone(&daemon), shutdown.clone(), signal_tx);

    let loop_daemon = Arc::clone(&daemon);
    let loop_handle = std::thread::spawn(move || loop_daemon.run_forever());

    // Accept IPC connections and watch for a local wake-from-sleep signal
    // (see `LOCAL_SUSPEND_MIN_MS`) on this thread, while the daemon's own
    // sequential loop runs on its own thread.
    let mut last_clocks: Option<(i64, i64)> = None;
    loop {
        if shutdown.load(Ordering::SeqCst) || loop_handle.is_finished() {
            break;
        }

        if let Some(ipc) = &mut ipc {
            ipc.accept_pending(&daemon);
        }

        if let (Ok(boot_ms), Ok(mono_ms)) =
            (platform.boot_clock_ms(), platform.monotonic_clock_ms())
        {
            if let Some((prev_boot_ms, prev_mono_ms)) = last_clocks {
                let suspend_ms = (boot_ms - prev_boot_ms) - (mono_ms - prev_mono_ms);
                if suspend_ms >= LOCAL_SUSPEND_MIN_MS {
                    daemon.flush_batch_now();
                }
            }
            last_clocks = Some((boot_ms, mono_ms));
        }

        tokio::select! {
            signal = signal_rx.recv() => {
                if signal.is_some() {
                    daemon.request_stop();
                }
                break;
            }
            _ = tokio::time::sleep(IPC_ACCEPT_POLL_INTERVAL) => {}
        }
    }

    daemon.request_stop();
    let _ = loop_handle.join();
    Ok(())
}

fn spawn_signal_handler(
    daemon: Arc<MacDaemon>,
    shutdown: Arc<AtomicBool>,
    signal_tx: mpsc::UnboundedSender<String>,
) {
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
        daemon.request_stop();
        let _ = signal_tx.send(signal_name.to_string());
    });
}
