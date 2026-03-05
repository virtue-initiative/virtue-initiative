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

use crate::capture::{capture_screen, is_session_unavailable_error};
use crate::config::{ClientPaths, load_state};
use crate::tray;

#[derive(Clone)]
struct LinuxDaemonHost {
    paths: ClientPaths,
    shutdown: Arc<AtomicBool>,
    pending_alert_events: Arc<Mutex<Vec<DaemonAlertEvent>>>,
}

impl LinuxDaemonHost {
    fn new(
        paths: ClientPaths,
        shutdown: Arc<AtomicBool>,
        pending_alert_events: Arc<Mutex<Vec<DaemonAlertEvent>>>,
    ) -> Self {
        Self {
            paths,
            shutdown,
            pending_alert_events,
        }
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

    fn should_stop(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
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
    let _tray = tray::start_daemon_tray(paths.clone());

    let shutdown = Arc::new(AtomicBool::new(false));
    let pending_alert_events = Arc::new(Mutex::new(vec![DaemonAlertEvent {
        kind: "daemon_start".to_string(),
        metadata: vec![("source".to_string(), "linux_service".to_string())],
        created_at: Utc::now(),
        device_id: None,
    }]));

    {
        let shutdown = shutdown.clone();
        let pending_alert_events = pending_alert_events.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{SignalKind, signal};

            let mut sigterm = match signal(SignalKind::terminate()) {
                Ok(s) => s,
                Err(_) => return,
            };
            let mut sigint = signal(SignalKind::interrupt()).ok();

            let signal_name = tokio::select! {
                _ = sigterm.recv() => "SIGTERM",
                _ = async {
                    match sigint.as_mut() {
                        Some(s) => s.recv().await,
                        None => std::future::pending::<Option<()>>().await,
                    }
                } => "SIGINT",
            };

            if let Ok(mut guard) = pending_alert_events.lock() {
                guard.push(DaemonAlertEvent {
                    kind: "daemon_stop_signal".to_string(),
                    metadata: vec![("signal".to_string(), signal_name.to_string())],
                    created_at: Utc::now(),
                    device_id: None,
                });
            }
            shutdown.store(true, Ordering::SeqCst);
        });
    }

    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let auth_client = AuthClient::new(token_store.clone())?;
    let api_client = ApiClient::new()?;
    let host = LinuxDaemonHost::new(paths.clone(), shutdown, pending_alert_events);

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
