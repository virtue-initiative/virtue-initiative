use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use tokio::sync::mpsc;
use virtue_core::{
    EventBus, IpcBridge, Ping, PlatformConfig, ProcessStarted, ProcessStopped,
    build_default_modules_reqwest, load_state, store_state,
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
            ipc.accept_pending(&mut bus, IpcBridge::forward_standard_inbound);
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
                    tokio::task::block_in_place(|| {
                        let _ = bus.send(ProcessStopped);
                        let _ = bus.send(Ping);
                        if let Ok(state) = bus.iter() {
                            let _ = store_state(&state_path, &state);
                        }
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
