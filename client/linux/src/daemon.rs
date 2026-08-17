use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use virtue_core::api::ReqwestApiClient;
use virtue_core::{Daemon, IpcBridge};

use crate::capture::LinuxPlatformHooks;
use crate::config::{ClientPaths, build_core_config};
use crate::tray;

type LinuxDaemon = Daemon<LinuxPlatformHooks, ReqwestApiClient>;

/// Installs the process-wide `tracing` subscriber. Captured on stdout, which
/// systemd's `Type=simple` unit forwards to journald — no new log file on
/// Linux. Honors `RUST_LOG` (useful for local `cargo run` too), falling back
/// to the build-type default directive shared with every other platform.
fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(virtue_core::logging::default_filter_directive(cfg!(
            debug_assertions
        )))
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stdout)
        .without_time()
        .init();
}

pub async fn run_daemon(paths: &ClientPaths) -> Result<()> {
    init_logging();
    paths.ensure_dirs()?;
    let _tray = tray::start_daemon_tray(paths.clone());

    let config = build_core_config(paths);
    let state_path = paths.state_dir.join("event_state.json");
    let daemon: Arc<LinuxDaemon> = Arc::new(tokio::task::block_in_place(|| {
        let api = ReqwestApiClient::new(&config)?;
        Daemon::new(config, LinuxPlatformHooks::new(), api, state_path)
    })?);

    let mut ipc = IpcBridge::bind(&paths.state_dir.join("daemon.sock"));

    spawn_signal_handler(Arc::clone(&daemon));

    let loop_daemon = Arc::clone(&daemon);
    let loop_handle = std::thread::spawn(move || loop_daemon.run_forever());

    // Accept newly-connected IPC clients on this thread while the daemon's
    // own sequential loop runs on its own, until that loop exits (shutdown).
    loop {
        if let Some(ipc) = &mut ipc {
            ipc.accept_pending(&daemon);
        }
        if loop_handle.is_finished() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = loop_handle.join();

    std::process::exit(0);
}

fn spawn_signal_handler(daemon: Arc<LinuxDaemon>) {
    tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(_) => return,
        };
        let mut sigint = signal(SignalKind::interrupt()).ok();

        tokio::select! {
            _ = sigterm.recv() => {}
            _ = async {
                match sigint.as_mut() {
                    Some(signal) => signal.recv().await,
                    None => std::future::pending::<Option<()>>().await,
                }
            } => {}
        };

        daemon.request_stop();
    });
}
