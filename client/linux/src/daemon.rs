use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use tokio::time::sleep;

use virtue_client_core::{
    ApiClient, AuthClient, BatchDaemonConfig, CaptureOutcome, CoreError, FileTokenStore,
    PersistedServiceState, ServiceEvent, ServiceHost, SleepOutcome, TokenStore, run_batch_daemon,
};

use crate::capture::{capture_screen, is_session_unavailable_error};
use crate::config::{ClientPaths, load_state};
use crate::tray;

#[derive(Clone)]
struct LinuxDaemonHost {
    paths: ClientPaths,
}

impl LinuxDaemonHost {
    fn new(paths: ClientPaths) -> Self {
        Self { paths }
    }
}

impl ServiceHost for LinuxDaemonHost {
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
        let state =
            load_state(&self.paths.state_file).map_err(|e| CoreError::Platform(e.to_string()))?;
        match capture_screen(state.backend_hint) {
            Ok(bytes) => Ok(CaptureOutcome::FramePng(bytes)),
            Err(err) if is_session_unavailable_error(&err) => {
                Ok(CaptureOutcome::SessionUnavailable)
            }
            Err(err) => Err(CoreError::Platform(err.to_string())),
        }
    }

    fn emit_event(&self, event: ServiceEvent) {
        match event {
            ServiceEvent::Info(msg) => eprintln!("daemon: {msg}"),
            ServiceEvent::Warn(msg) => eprintln!("daemon: {msg}"),
            ServiceEvent::Error(msg) => eprintln!("daemon: {msg}"),
        }
    }
}

pub async fn run_daemon(paths: &ClientPaths) -> Result<()> {
    paths.ensure_dirs()?;
    let _tray = tray::start_daemon_tray(paths.clone());

    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let auth_client = AuthClient::new(token_store.clone())?;
    let api_client = ApiClient::new()?;
    let host = LinuxDaemonHost::new(paths.clone());

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
        &paths.batch_buffer_file,
        config,
    )
    .await
    .map_err(Into::into)
}
