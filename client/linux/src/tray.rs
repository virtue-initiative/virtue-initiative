use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use ksni::blocking::TrayMethods;

use virtue_client_core::{FileTokenStore, TokenStore, build_default_tray_icon_rgba};

use crate::config::{ClientPaths, load_state};

const TOOLTIP_REFRESH_INTERVAL: Duration = Duration::from_secs(15);
const RETRY_INTERVAL: Duration = Duration::from_secs(30);
const LOG_THROTTLE_INTERVAL: Duration = Duration::from_secs(10 * 60);

pub struct DaemonTray {
    shutdown: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for DaemonTray {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn start_daemon_tray(paths: ClientPaths) -> Option<DaemonTray> {
    if std::env::var("VIRTUE_DISABLE_TRAY")
        .ok()
        .as_deref()
        .is_some_and(|v| matches!(v, "1" | "true" | "TRUE" | "yes" | "YES"))
    {
        return None;
    }

    let shutdown = Arc::new(AtomicBool::new(false));
    let worker = spawn_tray_worker(paths, shutdown.clone());

    Some(DaemonTray {
        shutdown,
        worker: Some(worker),
    })
}

fn spawn_tray_worker(paths: ClientPaths, shutdown: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut last_error_message: Option<String> = None;
        let mut last_error_log_at = std::time::Instant::now()
            .checked_sub(LOG_THROTTLE_INTERVAL)
            .unwrap_or_else(std::time::Instant::now);

        while !shutdown.load(Ordering::SeqCst) {
            if !has_session_bus() {
                sleep_interruptible(&shutdown, Duration::from_secs(5));
                continue;
            }

            match run_one_tray_session(&paths, &shutdown) {
                Ok(()) => break,
                Err(err) => {
                    let message = err.to_string();
                    let should_log = last_error_message.as_deref() != Some(message.as_str())
                        || last_error_log_at.elapsed() >= LOG_THROTTLE_INTERVAL;
                    if should_log {
                        eprintln!("tray unavailable (non-fatal): {message}");
                        last_error_message = Some(message);
                        last_error_log_at = std::time::Instant::now();
                    }
                }
            }

            sleep_interruptible(&shutdown, RETRY_INTERVAL);
        }
    })
}

fn run_one_tray_session(paths: &ClientPaths, shutdown: &Arc<AtomicBool>) -> anyhow::Result<()> {
    let mut tooltip = build_tooltip(paths);
    let tray = VirtueTray {
        tooltip: tooltip.clone(),
    };
    let handle = tray.spawn()?;

    let mut elapsed = Duration::ZERO;
    while !shutdown.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_secs(1));
        elapsed += Duration::from_secs(1);
        if elapsed < TOOLTIP_REFRESH_INTERVAL {
            continue;
        }
        elapsed = Duration::ZERO;

        let next = build_tooltip(paths);
        if next == tooltip {
            continue;
        }

        if handle
            .update(|tray| {
                tray.tooltip = next.clone();
            })
            .is_none()
        {
            return Err(anyhow::anyhow!("tray host disconnected"));
        }

        tooltip = next;
    }

    handle.shutdown().wait();
    Ok(())
}

fn build_tooltip(paths: &ClientPaths) -> String {
    let state = load_state(&paths.state_file).ok();
    let token_store = FileTokenStore::new(&paths.token_file);
    let has_token = token_store
        .get_access_token()
        .ok()
        .and_then(|token| token)
        .is_some();

    let logged_in = state
        .as_ref()
        .map(|s| s.monitoring_enabled && s.device_id.is_some())
        .unwrap_or(false)
        && has_token;

    if logged_in {
        let email = state
            .as_ref()
            .and_then(|s| s.email.as_deref())
            .unwrap_or("<unknown>");
        format!("Logged in as {email}. Run 'virtue' from a terminal to configure.")
    } else {
        "Not signed in. Run 'virtue login' from a terminal.".to_string()
    }
}

fn has_session_bus() -> bool {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
        return true;
    }

    let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") else {
        return false;
    };
    let bus_path = std::path::Path::new(&runtime_dir).join("bus");
    bus_path.exists()
}

fn sleep_interruptible(shutdown: &Arc<AtomicBool>, duration: Duration) {
    let mut elapsed = Duration::ZERO;
    while elapsed < duration && !shutdown.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_secs(1));
        elapsed += Duration::from_secs(1);
    }
}

#[derive(Clone, Debug)]
struct VirtueTray {
    tooltip: String,
}

impl ksni::Tray for VirtueTray {
    fn id(&self) -> String {
        "virtue".to_string()
    }

    fn title(&self) -> String {
        "Virtue".to_string()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![build_icon()]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Virtue".to_string(),
            description: self.tooltip.clone(),
            ..Default::default()
        }
    }
}

fn build_icon() -> ksni::Icon {
    fn fallback_icon() -> ksni::Icon {
        let (width, height, mut rgba) = build_default_tray_icon_rgba();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.rotate_right(1);
        }
        ksni::Icon {
            width: width as i32,
            height: height as i32,
            data: rgba,
        }
    }

    let decoded = match image::load_from_memory(include_bytes!("../assets/tray-icon.png")) {
        Ok(image) => image.into_rgba8(),
        Err(err) => {
            eprintln!("failed to decode tray icon image: {err}");
            return fallback_icon();
        }
    };

    let width = decoded.width() as i32;
    let height = decoded.height() as i32;
    let mut argb = decoded.into_raw();
    for pixel in argb.chunks_exact_mut(4) {
        pixel.rotate_right(1);
    }

    ksni::Icon {
        width,
        height,
        data: argb,
    }
}
