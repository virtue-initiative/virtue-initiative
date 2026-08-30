use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use ksni::blocking::TrayMethods;

use crate::config::{self, ClientPaths};

const TOOLTIP_REFRESH_INTERVAL: Duration = Duration::from_secs(15);
const RETRY_INTERVAL: Duration = Duration::from_secs(30);
const SESSION_BUS_WAIT_INTERVAL: Duration = Duration::from_secs(5);
const MISSING_WATCHER_RETRY_SCHEDULE: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(60),
];
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
        let mut missing_watcher_failures = 0_usize;

        while !shutdown.load(Ordering::SeqCst) {
            if !has_session_bus() {
                sleep_interruptible(&shutdown, SESSION_BUS_WAIT_INTERVAL);
                continue;
            }

            match run_one_tray_session(&paths, &shutdown) {
                Ok(()) => break,
                Err(err) => {
                    let message = err.message();
                    let should_log = last_error_message.as_deref() != Some(message.as_str())
                        || last_error_log_at.elapsed() >= LOG_THROTTLE_INTERVAL;
                    if should_log {
                        tracing::warn!("tray unavailable (non-fatal): {message}");
                        last_error_message = Some(message);
                        last_error_log_at = std::time::Instant::now();
                    }

                    let retry_delay = match err.kind() {
                        TrayErrorKind::MissingWatcher => {
                            let delay = next_missing_watcher_retry_delay(missing_watcher_failures);
                            missing_watcher_failures += 1;
                            delay
                        }
                        TrayErrorKind::Retryable => {
                            missing_watcher_failures = 0;
                            Some(RETRY_INTERVAL)
                        }
                    };

                    let Some(retry_delay) = retry_delay else {
                        tracing::warn!(
                            "tray unavailable (non-fatal): no tray host appeared after startup retries; giving up until the daemon restarts"
                        );
                        break;
                    };

                    sleep_interruptible(&shutdown, retry_delay);
                }
            }
        }
    })
}

fn next_missing_watcher_retry_delay(failure_count: usize) -> Option<Duration> {
    MISSING_WATCHER_RETRY_SCHEDULE.get(failure_count).copied()
}

fn run_one_tray_session(
    paths: &ClientPaths,
    shutdown: &Arc<AtomicBool>,
) -> Result<(), TraySessionError> {
    let mut tooltip = build_tooltip(paths);
    let tray = VirtueTray {
        tooltip: tooltip.clone(),
    };
    let handle = tray.spawn().map_err(TraySessionError::from_spawn_error)?;

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
            return Err(TraySessionError::retryable("tray host disconnected"));
        }

        tooltip = next;
    }

    handle.shutdown().wait();
    Ok(())
}

fn build_tooltip(paths: &ClientPaths) -> String {
    let status = config::load_service_status(paths).ok();

    let bin = match config::INSTANCE {
        Some(n) if !n.is_empty() => format!("virtue-{n}"),
        _ => "virtue".to_string(),
    };

    match status {
        Some(status) if status.is_authenticated => {
            let pending = status.pending_hash_count + status.pending_batch_count;
            let queue = if pending == 0 {
                "nothing queued".to_string()
            } else {
                format!("{pending} waiting to upload")
            };
            let account = status
                .account_email
                .unwrap_or_else(|| "signed in".to_string());
            format!("{account} — {queue}. Run '{bin} status' from a terminal for details.")
        }
        _ => format!("Not signed in. Run '{bin} login' from a terminal."),
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

fn classify_spawn_error(error: &ksni::Error) -> TrayErrorKind {
    match error {
        ksni::Error::Watcher(zbus::fdo::Error::ServiceUnknown(_)) => TrayErrorKind::MissingWatcher,
        _ => TrayErrorKind::Retryable,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrayErrorKind {
    MissingWatcher,
    Retryable,
}

#[derive(Debug)]
struct TraySessionError {
    kind: TrayErrorKind,
    message: String,
}

impl TraySessionError {
    fn from_spawn_error(error: ksni::Error) -> Self {
        Self {
            kind: classify_spawn_error(&error),
            message: error.to_string(),
        }
    }

    fn retryable(message: impl Into<String>) -> Self {
        Self {
            kind: TrayErrorKind::Retryable,
            message: message.into(),
        }
    }

    fn kind(&self) -> TrayErrorKind {
        self.kind
    }

    fn message(&self) -> String {
        self.message.clone()
    }
}

#[derive(Clone, Debug)]
struct VirtueTray {
    tooltip: String,
}

impl ksni::Tray for VirtueTray {
    fn id(&self) -> String {
        match config::INSTANCE {
            Some(n) if !n.is_empty() => format!("virtue-{n}"),
            _ => "virtue".to_string(),
        }
    }

    fn title(&self) -> String {
        match config::INSTANCE {
            Some(n) if !n.is_empty() => format!("Virtue ({n})"),
            _ => "Virtue".to_string(),
        }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![build_icon()]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Virtue - virtueinitiative.org".to_string(),
            description: self.tooltip.clone(),
            ..Default::default()
        }
    }
}

fn build_icon() -> ksni::Icon {
    fn fallback_icon() -> ksni::Icon {
        let width = 16_i32;
        let height = 16_i32;
        let mut argb = vec![0_u8; (width * height * 4) as usize];
        for pixel in argb.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[0xff, 0x20, 0x20, 0x20]);
        }
        ksni::Icon {
            width,
            height,
            data: argb,
        }
    }

    let decoded = match image::load_from_memory(include_bytes!("../assets/tray-icon.png")) {
        Ok(image) => image.into_rgba8(),
        Err(err) => {
            tracing::error!(error = %err, "failed to decode tray icon image");
            return fallback_icon();
        }
    };

    let width = decoded.width() as i32;
    let height = decoded.height() as i32;
    let mut argb = decoded.into_raw();
    for pixel in argb.as_chunks_mut::<4>().0 {
        pixel.rotate_right(1);
    }

    ksni::Icon {
        width,
        height,
        data: argb,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{TrayErrorKind, classify_spawn_error, next_missing_watcher_retry_delay};

    #[test]
    fn classifies_missing_status_notifier_watcher_as_non_retryable() {
        let error = ksni::Error::Watcher(zbus::fdo::Error::ServiceUnknown(
            "The name org.kde.StatusNotifierWatcher was not provided by any .service files"
                .to_string(),
        ));

        assert_eq!(classify_spawn_error(&error), TrayErrorKind::MissingWatcher);
    }

    #[test]
    fn leaves_other_spawn_errors_retryable() {
        let error = ksni::Error::WontShow;

        assert_eq!(classify_spawn_error(&error), TrayErrorKind::Retryable);
    }

    #[test]
    fn missing_watcher_retries_use_capped_startup_backoff() {
        assert_eq!(
            next_missing_watcher_retry_delay(0),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            next_missing_watcher_retry_delay(1),
            Some(Duration::from_secs(10))
        );
        assert_eq!(
            next_missing_watcher_retry_delay(2),
            Some(Duration::from_secs(60))
        );
        assert_eq!(next_missing_watcher_retry_delay(3), None);
    }

    #[test]
    fn tray_backoff_caps_at_max_schedule_entry() {
        for failure_count in 3..=10 {
            assert_eq!(
                next_missing_watcher_retry_delay(failure_count),
                None,
                "expected None for failure_count={failure_count}"
            );
        }
    }

    #[test]
    fn tray_backoff_schedule_is_strictly_increasing() {
        let delays: Vec<Duration> = (0..3)
            .map(|i| next_missing_watcher_retry_delay(i).unwrap())
            .collect();
        for window in delays.windows(2) {
            assert!(window[1] > window[0]);
        }
    }
}
