use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use tokio::time::sleep;

use virtue_client_core::{
    ApiClient, AuthClient, BatchDaemonConfig, CaptureOutcome, CoreError, DaemonAlertEvent,
    FileTokenStore, PersistedServiceState, ServiceEvent, ServiceHost, SleepOutcome, TokenStore,
    run_batch_daemon,
};

use crate::capture::{capture_screen, has_screen_capture_access, request_screen_capture_access};
use crate::config::{
    ClientPaths, ScreenshotPermissionStatus, load_daemon_status, load_state, save_daemon_status,
};

#[derive(Clone)]
struct MacDaemonHost {
    paths: ClientPaths,
    permission_prompt_requested: Arc<AtomicBool>,
    pending_alert_events: Arc<Mutex<Vec<DaemonAlertEvent>>>,
}

impl MacDaemonHost {
    fn new(paths: ClientPaths, pending_alert_events: Arc<Mutex<Vec<DaemonAlertEvent>>>) -> Self {
        Self {
            paths,
            permission_prompt_requested: Arc::new(AtomicBool::new(false)),
            pending_alert_events,
        }
    }
}

impl ServiceHost for MacDaemonHost {
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
        sleep(duration).await;
        Ok(SleepOutcome::Elapsed)
    }

    async fn capture_frame_png(&self) -> virtue_client_core::CoreResult<CaptureOutcome> {
        if !has_screen_capture_access() {
            if !self
                .permission_prompt_requested
                .swap(true, Ordering::SeqCst)
            {
                let _ = request_screen_capture_access();
            }
            let error_text = "screen recording permission missing for daemon process".to_string();
            update_daemon_status(
                &self.paths,
                ScreenshotPermissionStatus::Missing,
                Some(error_text),
            );
            return Ok(CaptureOutcome::PermissionMissing);
        }

        self.permission_prompt_requested
            .store(false, Ordering::SeqCst);

        match capture_screen() {
            Ok(bytes) => {
                update_daemon_status(&self.paths, ScreenshotPermissionStatus::Granted, None);
                Ok(CaptureOutcome::FramePng(bytes))
            }
            Err(err) => {
                let error_text = format!("{err:#}");
                let screenshot_permission = if is_permission_missing_error(&error_text) {
                    ScreenshotPermissionStatus::Missing
                } else {
                    ScreenshotPermissionStatus::Unknown
                };
                update_daemon_status(&self.paths, screenshot_permission, Some(error_text.clone()));
                if screenshot_permission == ScreenshotPermissionStatus::Missing {
                    Ok(CaptureOutcome::PermissionMissing)
                } else {
                    Err(CoreError::Platform(error_text))
                }
            }
        }
    }

    fn emit_event(&self, event: ServiceEvent) {
        match event {
            ServiceEvent::Info(msg) => eprintln!("daemon: {msg}"),
            ServiceEvent::Warn(msg) => eprintln!("daemon: {msg}"),
            ServiceEvent::Error(msg) => eprintln!("daemon: {msg}"),
        }
    }

    fn drain_alert_events(&self) -> virtue_client_core::CoreResult<Vec<DaemonAlertEvent>> {
        let mut guard = self
            .pending_alert_events
            .lock()
            .map_err(|_| CoreError::Platform("alert event queue lock poisoned".to_string()))?;
        Ok(std::mem::take(&mut *guard))
    }
}

pub async fn run_daemon(paths: &ClientPaths) -> Result<()> {
    paths.ensure_dirs()?;
    let pending_alert_events = Arc::new(Mutex::new(vec![DaemonAlertEvent {
        kind: "daemon_start".to_string(),
        metadata: vec![("source".to_string(), "macos_launch_agent".to_string())],
        created_at: Utc::now(),
        device_id: None,
    }]));

    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let auth_client = AuthClient::new(token_store.clone())?;
    let api_client = ApiClient::new()?;
    let host = MacDaemonHost::new(paths.clone(), pending_alert_events);

    let config = BatchDaemonConfig {
        settings_refresh_interval: Duration::from_secs(30 * 60),
        settings_fetch_retry_interval: Duration::from_secs(20),
        idle_retry_interval: Duration::from_secs(20),
        token_refresh_threshold: Duration::from_secs(120),
        session_unavailable_log_interval: Duration::from_secs(5 * 60),
        continue_on_token_refresh_error: false,
    };

    run_batch_daemon(
        &host,
        token_store,
        &auth_client,
        &api_client,
        &paths.queue_file,
        config,
    )
    .await
    .map_err(Into::into)
}

fn update_daemon_status(
    paths: &ClientPaths,
    screenshot_permission: ScreenshotPermissionStatus,
    last_error: Option<String>,
) {
    let mut status = load_daemon_status(&paths.daemon_status_file).unwrap_or_default();
    status.screenshot_permission = screenshot_permission;
    status.last_error = last_error;
    status.updated_at = Some(Utc::now().to_rfc3339());
    let _ = save_daemon_status(&paths.daemon_status_file, &status);
}

fn is_permission_missing_error(error_text: &str) -> bool {
    let normalized = error_text.to_ascii_lowercase();
    normalized.contains("screen recording")
        || normalized.contains("not authorized")
        || normalized.contains("operation not permitted")
        || normalized.contains("permission denied")
}
