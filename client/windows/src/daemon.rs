use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::Utc;
use tokio::time::sleep;
use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, HANDLE};
use windows::Win32::System::Threading::{MUTEX_MODIFY_STATE, OpenMutexW};
use windows::core::w;

use virtue_client_core::{
    ApiClient, AuthClient, BatchDaemonConfig, CaptureOutcome, CoreError, FileTokenStore,
    PersistedServiceState, ServiceEvent, ServiceHost, SleepOutcome, TokenStore, run_batch_daemon,
};

use crate::capture::capture_screen_png;
use crate::config::{ClientPaths, load_state};
use crate::service_log::ServiceLogger;

const TRAY_ENSURE_INTERVAL: Duration = Duration::from_secs(30);

struct WindowsDaemonHost<'a> {
    paths: ClientPaths,
    shutdown: Arc<AtomicBool>,
    logger: &'a ServiceLogger,
    last_tray_ensure: Mutex<Option<Instant>>,
}

impl<'a> WindowsDaemonHost<'a> {
    fn new(paths: ClientPaths, shutdown: Arc<AtomicBool>, logger: &'a ServiceLogger) -> Self {
        Self {
            paths,
            shutdown,
            logger,
            last_tray_ensure: Mutex::new(None),
        }
    }
}

impl ServiceHost for WindowsDaemonHost<'_> {
    fn load_persisted_state(&self) -> virtue_client_core::CoreResult<PersistedServiceState> {
        let state =
            load_state(&self.paths.state_file).map_err(|e| CoreError::Platform(e.to_string()))?;
        Ok(PersistedServiceState {
            monitoring_enabled: state.monitoring_enabled,
            device_id: state.device_id,
        })
    }

    fn now_utc(&self) -> chrono::DateTime<Utc> {
        Utc::now()
    }

    async fn sleep_interruptible(
        &self,
        duration: Duration,
    ) -> virtue_client_core::CoreResult<SleepOutcome> {
        let mut remaining = duration;
        while remaining > Duration::ZERO {
            if self.should_stop() {
                return Ok(SleepOutcome::Interrupted);
            }
            let tick = remaining.min(Duration::from_secs(1));
            sleep(tick).await;
            remaining = remaining.saturating_sub(tick);
        }
        if self.should_stop() {
            Ok(SleepOutcome::Interrupted)
        } else {
            Ok(SleepOutcome::Elapsed)
        }
    }

    async fn capture_frame_png(&self) -> virtue_client_core::CoreResult<CaptureOutcome> {
        capture_screen_png()
            .map(CaptureOutcome::FramePng)
            .map_err(|e| CoreError::Platform(e.to_string()))
    }

    fn emit_event(&self, event: ServiceEvent) {
        match event {
            ServiceEvent::Info(msg) => self.logger.info(&msg),
            ServiceEvent::Warn(msg) => self.logger.warn(&msg),
            ServiceEvent::Error(msg) => self.logger.error(&msg),
        }
    }

    fn should_stop(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    fn on_loop_tick(&self) -> virtue_client_core::CoreResult<()> {
        let mut guard = self
            .last_tray_ensure
            .lock()
            .map_err(|_| CoreError::Platform("tray ensure lock poisoned".to_string()))?;
        let should_ensure = guard
            .map(|when| when.elapsed() >= TRAY_ENSURE_INTERVAL)
            .unwrap_or(true);
        if should_ensure {
            ensure_tray_running(self.logger);
            *guard = Some(Instant::now());
        }
        Ok(())
    }
}

pub async fn run_daemon(shutdown: Arc<AtomicBool>, logger: &ServiceLogger) -> Result<()> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;

    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let auth_client = AuthClient::new(token_store.clone())?;
    let api_client = ApiClient::new()?;
    let host = WindowsDaemonHost::new(paths.clone(), shutdown, logger);

    let config = BatchDaemonConfig {
        settings_refresh_interval: Duration::from_secs(30 * 60),
        settings_fetch_retry_interval: Duration::from_secs(30),
        idle_retry_interval: Duration::from_secs(30),
        token_refresh_threshold: Duration::from_secs(120),
        session_unavailable_log_interval: Duration::from_secs(5 * 60),
        continue_on_token_refresh_error: true,
    };

    run_batch_daemon(
        &host,
        token_store,
        &auth_client,
        &api_client,
        &paths.batch_buffer_file,
        config,
    )
    .await
    .map_err(Into::into)
}

fn ensure_tray_running(logger: &ServiceLogger) {
    if is_tray_running() {
        return;
    }

    let tray_path = match std::env::current_exe() {
        Ok(path) => path.with_file_name("virtue-tray.exe"),
        Err(err) => {
            logger.warn(&format!("cannot resolve tray path from current exe: {err}"));
            return;
        }
    };

    if !tray_path.exists() {
        logger.warn(&format!("tray executable missing: {}", tray_path.display()));
        return;
    }

    match Command::new(&tray_path).spawn() {
        Ok(_) => logger.info("tray process launch requested by daemon"),
        Err(err) => logger.warn(&format!("failed to launch tray process: {err}")),
    }
}

fn is_tray_running() -> bool {
    unsafe {
        let handle: Result<HANDLE, _> =
            OpenMutexW(MUTEX_MODIFY_STATE, false, w!("Local\\VirtueTrayInstance"));
        match handle {
            Ok(handle) => {
                let _ = CloseHandle(handle);
                true
            }
            Err(err) => err.code() != ERROR_FILE_NOT_FOUND.into(),
        }
    }
}
