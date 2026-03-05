use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api_client::{ApiClient, Device};
use crate::auth::AuthClient;
use crate::batch::{BatchBlob, BatchItem};
use crate::error::CoreResult;
use crate::image_pipeline::ImagePipeline;
use crate::schedule::{CaptureSchedulePolicy, CaptureScheduleState};
use crate::service_host::{CaptureOutcome, ServiceEvent, ServiceHost, SleepOutcome};
use crate::token_store::TokenStore;
use crate::upload::{UploadClient, UploadClientConfig, uuid_str_to_bytes};
use crate::{resolve_batch_window_seconds, resolve_capture_interval_seconds};

#[derive(Clone, Debug)]
pub struct BatchDaemonConfig {
    pub settings_refresh_interval: Duration,
    pub settings_fetch_retry_interval: Duration,
    pub idle_retry_interval: Duration,
    pub token_refresh_threshold: Duration,
    pub session_unavailable_log_interval: Duration,
    pub continue_on_token_refresh_error: bool,
}

impl Default for BatchDaemonConfig {
    fn default() -> Self {
        Self {
            settings_refresh_interval: Duration::from_secs(30 * 60),
            settings_fetch_retry_interval: Duration::from_secs(30),
            idle_retry_interval: Duration::from_secs(30),
            token_refresh_threshold: Duration::from_secs(120),
            session_unavailable_log_interval: Duration::from_secs(5 * 60),
            continue_on_token_refresh_error: true,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct BatchBuffer {
    items: Vec<BatchItem>,
    window_start: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingAlertLog {
    kind: String,
    metadata: Vec<(String, String)>,
    created_at: DateTime<Utc>,
    device_id: Option<String>,
}

fn load_batch_buffer(path: &Path) -> BatchBuffer {
    fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_batch_buffer(path: &Path, buf: &BatchBuffer) {
    if let Ok(bytes) = serde_json::to_vec(buf) {
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, bytes).is_ok() {
            let _ = fs::rename(tmp, path);
        }
    }
}

fn alert_log_queue_path(batch_buffer_path: &Path) -> std::path::PathBuf {
    batch_buffer_path.with_file_name("alert_logs_queue.json")
}

fn load_alert_log_queue(path: &Path) -> VecDeque<PendingAlertLog> {
    fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_alert_log_queue(path: &Path, queue: &VecDeque<PendingAlertLog>) {
    if let Ok(bytes) = serde_json::to_vec(queue) {
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, bytes).is_ok() {
            let _ = fs::rename(tmp, path);
        }
    }
}

pub async fn run_batch_daemon<H: ServiceHost>(
    host: &H,
    token_store: Arc<dyn TokenStore>,
    auth_client: &AuthClient,
    api_client: &ApiClient,
    batch_buffer_path: &Path,
    config: BatchDaemonConfig,
) -> CoreResult<()> {
    let pipeline = ImagePipeline;
    let mut upload_client = UploadClient::new()?;

    let mut schedule_state = CaptureScheduleState::default();
    let mut last_cycle_success = true;
    let mut warned_missing_e2ee = false;
    let mut device_settings: Option<Device> = None;
    let mut last_settings_fetch: Option<Instant> = None;
    let mut last_settings_attempt: Option<Instant> = None;
    let mut last_hash_server_fetch: Option<Instant> = None;
    let mut last_session_unavailable_log: Option<Instant> = None;
    let mut batch_buffer = load_batch_buffer(batch_buffer_path);
    let mut batch_window_start: DateTime<Utc> = batch_buffer.window_start.unwrap_or_else(Utc::now);
    let alert_queue_path = alert_log_queue_path(batch_buffer_path);
    let mut alert_queue = load_alert_log_queue(&alert_queue_path);
    let mut stopping = false;

    emit_info(host, "capture daemon started");

    if let Some(access_token) = token_store.get_access_token()?
        && let Err(err) = auth_client.fetch_and_decrypt_e2ee_key(&access_token).await
    {
        emit_warn(
            host,
            &format!("could not fetch E2EE key on startup: {err:#}"),
        );
    }

    loop {
        host.on_loop_tick()?;

        let state = match host.load_persisted_state() {
            Ok(state) => state,
            Err(err) => {
                emit_warn(host, &format!("state read failed: {err}"));
                if stopping {
                    break;
                }
                if sleep_or_stop(host, config.idle_retry_interval).await? {
                    stopping = true;
                }
                continue;
            }
        };

        if collect_alert_events(host, &mut alert_queue, state.device_id.as_deref())? {
            save_alert_log_queue(&alert_queue_path, &alert_queue);
        }

        let access_token_opt = match token_store.get_access_token() {
            Ok(token) => token,
            Err(err) => {
                emit_warn(host, &format!("token read failed: {err}"));
                if stopping {
                    break;
                }
                if sleep_or_stop(host, config.idle_retry_interval).await? {
                    stopping = true;
                }
                continue;
            }
        };

        if let (Some(access_token), Some(device_id)) =
            (access_token_opt.as_deref(), state.device_id.as_deref())
        {
            flush_alert_log_queue(
                host,
                api_client,
                access_token,
                device_id,
                &mut alert_queue,
                &alert_queue_path,
            )
            .await;
        }

        if stopping || host.should_stop() {
            break;
        }

        let Some(mut access_token) = access_token_opt else {
            if sleep_or_stop(host, config.idle_retry_interval).await? {
                stopping = true;
            }
            continue;
        };

        let Some(device_id) = state.device_id.clone() else {
            if sleep_or_stop(host, config.idle_retry_interval).await? {
                stopping = true;
            }
            continue;
        };
        let Some(e2ee_key) = (match token_store.get_e2ee_key() {
            Ok(key) => key,
            Err(err) => {
                emit_warn(host, &format!("e2ee key read failed: {err}"));
                if sleep_or_stop(host, config.idle_retry_interval).await? {
                    stopping = true;
                }
                continue;
            }
        }) else {
            if !warned_missing_e2ee {
                emit_warn(
                    host,
                    "E2EE key not set; sign in again to derive and store it",
                );
                warned_missing_e2ee = true;
            }
            if sleep_or_stop(host, config.idle_retry_interval).await? {
                stopping = true;
            }
            continue;
        };
        warned_missing_e2ee = false;

        if !state.monitoring_enabled {
            if sleep_or_stop(host, config.idle_retry_interval).await? {
                stopping = true;
            }
            continue;
        }

        match auth_client
            .refresh_access_token_if_needed(config.token_refresh_threshold)
            .await
        {
            Ok(Some(refreshed)) => access_token = refreshed.access_token,
            Ok(None) => {}
            Err(err) => {
                emit_warn(host, &format!("token refresh failed: {err}"));
                if !config.continue_on_token_refresh_error {
                    if sleep_or_stop(host, config.idle_retry_interval).await? {
                        stopping = true;
                    }
                    continue;
                }
            }
        }

        let attempt_settings_refresh = match last_settings_attempt {
            Some(last) => last.elapsed() >= config.settings_fetch_retry_interval,
            None => true,
        };
        let needs_settings_refresh = last_settings_fetch
            .map(|t| t.elapsed() >= config.settings_refresh_interval)
            .unwrap_or(true);

        if attempt_settings_refresh && (device_settings.is_none() || needs_settings_refresh) {
            last_settings_attempt = Some(Instant::now());
            match api_client.get_device(&access_token, &device_id).await {
                Ok(settings) => {
                    device_settings = Some(settings);
                    last_settings_fetch = Some(Instant::now());
                }
                Err(err) => emit_warn(host, &format!("device settings fetch failed: {err}")),
            }
        }

        if last_hash_server_fetch
            .map(|t| t.elapsed() >= config.settings_refresh_interval)
            .unwrap_or(true)
        {
            match api_client
                .get_hash_server_url(&access_token, &device_id)
                .await
            {
                Ok(hash_url) => {
                    match UploadClient::with_config(UploadClientConfig {
                        hash_base_url: Some(hash_url),
                        ..UploadClientConfig::default()
                    }) {
                        Ok(client) => {
                            upload_client = client;
                            last_hash_server_fetch = Some(Instant::now());
                        }
                        Err(err) => {
                            emit_warn(host, &format!("failed to build upload client: {err}"))
                        }
                    }
                }
                Err(err) => emit_warn(host, &format!("hash server URL fetch failed: {err}")),
            }
        }

        let capture_enabled = device_settings
            .as_ref()
            .map(|settings| settings.enabled)
            .unwrap_or(state.monitoring_enabled);
        if !capture_enabled {
            if sleep_or_stop(host, config.idle_retry_interval).await? {
                stopping = true;
            }
            continue;
        }

        let policy = CaptureSchedulePolicy {
            base_interval: Duration::from_secs(resolve_capture_interval_seconds()),
            ..CaptureSchedulePolicy::default()
        };
        let mut rng = thread_rng();
        let delay = policy.next_delay(&mut schedule_state, last_cycle_success, &mut rng);
        if sleep_or_stop(host, delay).await? {
            stopping = true;
            continue;
        }

        let now = host.now_utc();
        let window_age_secs = (now - batch_window_start).num_seconds().max(0) as u64;
        if window_age_secs >= resolve_batch_window_seconds() && !batch_buffer.items.is_empty() {
            let end_time = now;
            let start_time = batch_window_start;

            let items = std::mem::take(&mut batch_buffer.items);
            let blob = BatchBlob::new(items.clone());

            match upload_client
                .upload_batch(
                    &access_token,
                    &device_id,
                    &blob,
                    start_time,
                    end_time,
                    &e2ee_key,
                )
                .await
            {
                Ok(resp) => {
                    emit_info(host, &format!("batch uploaded: {}", resp.batch.id));
                    batch_buffer = BatchBuffer::default();
                    batch_window_start = now;
                }
                Err(err) => {
                    emit_warn(host, &format!("batch upload failed: {err}"));
                    batch_buffer.items = items;
                }
            }

            save_batch_buffer(batch_buffer_path, &batch_buffer);
            continue;
        }

        let raw_capture = match host.capture_frame_png().await {
            Ok(CaptureOutcome::FramePng(bytes)) => bytes,
            Ok(CaptureOutcome::SessionUnavailable) => {
                last_cycle_success = true;
                let should_log = last_session_unavailable_log
                    .map(|when| when.elapsed() >= config.session_unavailable_log_interval)
                    .unwrap_or(true);
                if should_log {
                    emit_warn(host, "session unavailable for capture");
                    last_session_unavailable_log = Some(Instant::now());
                }
                if sleep_or_stop(host, config.idle_retry_interval).await? {
                    stopping = true;
                }
                continue;
            }
            Ok(CaptureOutcome::PermissionMissing) => {
                last_cycle_success = false;
                let item =
                    make_missed_capture(now, "permission_missing", "capture permission missing");
                upload_hash_for_item(host, &upload_client, &access_token, &device_id, &item).await;
                batch_buffer.items.push(item);
                if batch_buffer.window_start.is_none() {
                    batch_buffer.window_start = Some(batch_window_start);
                }
                save_batch_buffer(batch_buffer_path, &batch_buffer);
                continue;
            }
            Err(err) => {
                last_cycle_success = false;
                emit_warn(host, &format!("capture failed: {err}"));
                let item = make_missed_capture(now, "capture_failed", &err.to_string());
                upload_hash_for_item(host, &upload_client, &access_token, &device_id, &item).await;
                batch_buffer.items.push(item);
                if batch_buffer.window_start.is_none() {
                    batch_buffer.window_start = Some(batch_window_start);
                }
                save_batch_buffer(batch_buffer_path, &batch_buffer);
                continue;
            }
        };

        let processed = match pipeline.process(&raw_capture) {
            Ok(output) => output,
            Err(err) => {
                last_cycle_success = false;
                emit_warn(host, &format!("image pipeline failed: {err}"));
                let item = make_missed_capture(now, "image_pipeline_failed", &err.to_string());
                upload_hash_for_item(host, &upload_client, &access_token, &device_id, &item).await;
                batch_buffer.items.push(item);
                if batch_buffer.window_start.is_none() {
                    batch_buffer.window_start = Some(batch_window_start);
                }
                save_batch_buffer(batch_buffer_path, &batch_buffer);
                continue;
            }
        };

        let item = BatchItem {
            id: Uuid::new_v4().to_string(),
            taken_at: now.timestamp_millis(),
            kind: "screenshot".to_string(),
            image: Some(processed.bytes),
            metadata: vec![],
        };
        upload_hash_for_item(host, &upload_client, &access_token, &device_id, &item).await;

        batch_buffer.items.push(item);
        if batch_buffer.window_start.is_none() {
            batch_buffer.window_start = Some(batch_window_start);
        }
        save_batch_buffer(batch_buffer_path, &batch_buffer);
        last_cycle_success = true;
    }

    emit_info(host, "capture daemon stopping");
    Ok(())
}

async fn sleep_or_stop<H: ServiceHost>(host: &H, duration: Duration) -> CoreResult<bool> {
    if host.should_stop() {
        return Ok(true);
    }

    match host.sleep_interruptible(duration).await? {
        SleepOutcome::Interrupted => Ok(true),
        SleepOutcome::Elapsed => Ok(host.should_stop()),
    }
}

fn make_missed_capture(now: DateTime<Utc>, reason: &str, error: &str) -> BatchItem {
    BatchItem {
        id: Uuid::new_v4().to_string(),
        taken_at: now.timestamp_millis(),
        kind: "missed_capture".to_string(),
        image: None,
        metadata: vec![
            ("reason".to_string(), reason.to_string()),
            ("error".to_string(), error.to_string()),
        ],
    }
}

fn collect_alert_events<H: ServiceHost>(
    host: &H,
    queue: &mut VecDeque<PendingAlertLog>,
    default_device_id: Option<&str>,
) -> CoreResult<bool> {
    let mut changed = false;
    for event in host.drain_alert_events()? {
        let device_id = event
            .device_id
            .or_else(|| default_device_id.map(ToString::to_string));

        if device_id.is_none() {
            emit_warn(
                host,
                &format!(
                    "dropping alert event '{}' because no device_id is available",
                    event.kind
                ),
            );
            continue;
        }

        queue.push_back(PendingAlertLog {
            kind: event.kind,
            metadata: event.metadata,
            created_at: event.created_at,
            device_id,
        });
        changed = true;
    }
    Ok(changed)
}

async fn flush_alert_log_queue<H: ServiceHost>(
    host: &H,
    api_client: &ApiClient,
    access_token: &str,
    default_device_id: &str,
    queue: &mut VecDeque<PendingAlertLog>,
    queue_path: &Path,
) {
    while let Some(next) = queue.front().cloned() {
        let device_id = next
            .device_id
            .as_deref()
            .unwrap_or(default_device_id)
            .to_string();

        match api_client
            .create_alert_log(
                access_token,
                &device_id,
                &next.kind,
                &next.metadata,
                next.created_at,
            )
            .await
        {
            Ok(()) => {
                let _ = queue.pop_front();
                save_alert_log_queue(queue_path, queue);
            }
            Err(err) => {
                emit_warn(host, &format!("alert log upload failed: {err}"));
                break;
            }
        }
    }
}

async fn upload_hash_for_item<H: ServiceHost>(
    host: &H,
    upload_client: &UploadClient,
    access_token: &str,
    device_id: &str,
    item: &BatchItem,
) {
    let Some(device_id_bytes) = uuid_str_to_bytes(device_id) else {
        emit_warn(host, "content hash upload skipped: invalid device id");
        return;
    };

    if let Err(err) = upload_client
        .upload_hash(access_token, &device_id_bytes, &item.sha256())
        .await
    {
        emit_warn(host, &format!("content hash upload failed: {err}"));
    }
}

fn emit_info<H: ServiceHost>(host: &H, msg: &str) {
    host.emit_event(ServiceEvent::Info(msg.to_string()));
}

fn emit_warn<H: ServiceHost>(host: &H, msg: &str) {
    host.emit_event(ServiceEvent::Warn(msg.to_string()));
}

#[allow(dead_code)]
fn emit_error<H: ServiceHost>(host: &H, msg: &str) {
    host.emit_event(ServiceEvent::Error(msg.to_string()));
}
