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

#[derive(Debug)]
struct RuntimeState {
    schedule_state: CaptureScheduleState,
    last_cycle_success: bool,
    warned_missing_e2ee: bool,
    device_settings: Option<Device>,
    last_settings_fetch: Option<Instant>,
    last_settings_attempt: Option<Instant>,
    last_hash_server_fetch: Option<Instant>,
    last_session_unavailable_log: Option<Instant>,
    batch_buffer: BatchBuffer,
    batch_window_start: DateTime<Utc>,
    alert_queue: VecDeque<PendingAlertLog>,
    stopping: bool,
}

impl RuntimeState {
    fn new(batch_buffer_path: &Path) -> Self {
        let batch_buffer = load_batch_buffer(batch_buffer_path);
        let batch_window_start = batch_buffer.window_start.unwrap_or_else(Utc::now);
        let alert_queue = load_alert_log_queue(&alert_log_queue_path(batch_buffer_path));

        Self {
            schedule_state: CaptureScheduleState::default(),
            last_cycle_success: true,
            warned_missing_e2ee: false,
            device_settings: None,
            last_settings_fetch: None,
            last_settings_attempt: None,
            last_hash_server_fetch: None,
            last_session_unavailable_log: None,
            batch_buffer,
            batch_window_start,
            alert_queue,
            stopping: false,
        }
    }
}

struct CycleInputs {
    access_token: String,
    device_id: String,
    e2ee_key: [u8; 32],
}

struct DaemonContext<'a, H: ServiceHost> {
    host: &'a H,
    token_store: &'a Arc<dyn TokenStore>,
    auth_client: &'a AuthClient,
    api_client: &'a ApiClient,
    pipeline: &'a ImagePipeline,
    batch_buffer_path: &'a Path,
    alert_queue_path: &'a Path,
    config: &'a BatchDaemonConfig,
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
    let alert_queue_path = alert_log_queue_path(batch_buffer_path);
    let mut runtime = RuntimeState::new(batch_buffer_path);
    let ctx = DaemonContext {
        host,
        token_store: &token_store,
        auth_client,
        api_client,
        pipeline: &pipeline,
        batch_buffer_path,
        alert_queue_path: &alert_queue_path,
        config: &config,
    };

    emit_info(ctx.host, "capture daemon started");

    if let Some(access_token) = token_store.get_access_token()?
        && let Err(err) = auth_client.fetch_and_decrypt_e2ee_key(&access_token).await
    {
        emit_warn(
            ctx.host,
            &format!("could not fetch E2EE key on startup: {err:#}"),
        );
    }

    loop {
        let should_stop = run_iteration(&ctx, &mut upload_client, &mut runtime).await?;
        if should_stop {
            break;
        }
    }

    if !runtime.alert_queue.is_empty()
        && let (Some(access_token), Some(device_id)) = (
            token_store.get_access_token()?,
            ctx.host.load_persisted_state()?.device_id,
        )
    {
        flush_alert_log_queue(
            ctx.host,
            ctx.api_client,
            &access_token,
            &device_id,
            &mut runtime.alert_queue,
            ctx.alert_queue_path,
        )
        .await;
    }

    emit_info(ctx.host, "capture daemon stopping");
    Ok(())
}

async fn run_iteration<H: ServiceHost>(
    ctx: &DaemonContext<'_, H>,
    upload_client: &mut UploadClient,
    runtime: &mut RuntimeState,
) -> CoreResult<bool> {
    ctx.host.on_loop_tick()?;

    let Some(state) = load_state_or_retry(ctx.host, ctx.config, runtime).await? else {
        return Ok(runtime.stopping || ctx.host.should_stop());
    };

    if collect_alert_events(
        ctx.host,
        &mut runtime.alert_queue,
        state.device_id.as_deref(),
    )? {
        save_alert_log_queue(ctx.alert_queue_path, &runtime.alert_queue);
    }

    let access_token_opt =
        load_access_token_or_retry(ctx.host, ctx.token_store, ctx.config, runtime).await?;
    if let (Some(access_token), Some(device_id)) =
        (access_token_opt.as_deref(), state.device_id.as_deref())
    {
        flush_alert_log_queue(
            ctx.host,
            ctx.api_client,
            access_token,
            device_id,
            &mut runtime.alert_queue,
            ctx.alert_queue_path,
        )
        .await;
    }

    if runtime.stopping || ctx.host.should_stop() {
        return Ok(true);
    }

    let Some(mut cycle) = build_cycle_inputs(
        ctx.host,
        ctx.token_store,
        ctx.auth_client,
        access_token_opt,
        &state,
        ctx.config,
        runtime,
    )
    .await?
    else {
        return Ok(false);
    };

    refresh_control_plane(
        ctx.host,
        ctx.api_client,
        upload_client,
        &cycle,
        state.monitoring_enabled,
        ctx.config,
        runtime,
    )
    .await;

    if !capture_enabled(runtime, state.monitoring_enabled) {
        mark_stop_if_sleep_interrupted(ctx.host, ctx.config.idle_retry_interval, runtime).await?;
        return Ok(false);
    }

    if wait_for_next_capture(ctx.host, runtime).await? {
        return Ok(false);
    }

    let now = ctx.host.now_utc();
    if flush_batch_if_due(
        ctx.host,
        upload_client,
        &cycle,
        now,
        ctx.batch_buffer_path,
        runtime,
    )
    .await
    {
        return Ok(false);
    }

    process_capture(ctx, upload_client, &cycle, now, runtime).await?;

    cycle.access_token.clear();
    Ok(false)
}

async fn load_state_or_retry<H: ServiceHost>(
    host: &H,
    config: &BatchDaemonConfig,
    runtime: &mut RuntimeState,
) -> CoreResult<Option<crate::service_host::PersistedServiceState>> {
    match host.load_persisted_state() {
        Ok(state) => Ok(Some(state)),
        Err(err) => {
            emit_warn(host, &format!("state read failed: {err}"));
            if runtime.stopping {
                return Ok(None);
            }
            mark_stop_if_sleep_interrupted(host, config.idle_retry_interval, runtime).await?;
            Ok(None)
        }
    }
}

async fn load_access_token_or_retry<H: ServiceHost>(
    host: &H,
    token_store: &Arc<dyn TokenStore>,
    config: &BatchDaemonConfig,
    runtime: &mut RuntimeState,
) -> CoreResult<Option<String>> {
    match token_store.get_access_token() {
        Ok(token) => Ok(token),
        Err(err) => {
            emit_warn(host, &format!("token read failed: {err}"));
            if runtime.stopping {
                return Ok(None);
            }
            mark_stop_if_sleep_interrupted(host, config.idle_retry_interval, runtime).await?;
            Ok(None)
        }
    }
}

async fn build_cycle_inputs<H: ServiceHost>(
    host: &H,
    token_store: &Arc<dyn TokenStore>,
    auth_client: &AuthClient,
    access_token_opt: Option<String>,
    state: &crate::service_host::PersistedServiceState,
    config: &BatchDaemonConfig,
    runtime: &mut RuntimeState,
) -> CoreResult<Option<CycleInputs>> {
    let Some(mut access_token) = access_token_opt else {
        mark_stop_if_sleep_interrupted(host, config.idle_retry_interval, runtime).await?;
        return Ok(None);
    };

    let Some(device_id) = state.device_id.clone() else {
        mark_stop_if_sleep_interrupted(host, config.idle_retry_interval, runtime).await?;
        return Ok(None);
    };

    let Some(e2ee_key) = (match token_store.get_e2ee_key() {
        Ok(key) => key,
        Err(err) => {
            emit_warn(host, &format!("e2ee key read failed: {err}"));
            mark_stop_if_sleep_interrupted(host, config.idle_retry_interval, runtime).await?;
            return Ok(None);
        }
    }) else {
        if !runtime.warned_missing_e2ee {
            emit_warn(
                host,
                "E2EE key not set; sign in again to derive and store it",
            );
            runtime.warned_missing_e2ee = true;
        }
        mark_stop_if_sleep_interrupted(host, config.idle_retry_interval, runtime).await?;
        return Ok(None);
    };
    runtime.warned_missing_e2ee = false;

    if !state.monitoring_enabled {
        mark_stop_if_sleep_interrupted(host, config.idle_retry_interval, runtime).await?;
        return Ok(None);
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
                mark_stop_if_sleep_interrupted(host, config.idle_retry_interval, runtime).await?;
                return Ok(None);
            }
        }
    }

    Ok(Some(CycleInputs {
        access_token,
        device_id,
        e2ee_key,
    }))
}

async fn refresh_control_plane<H: ServiceHost>(
    host: &H,
    api_client: &ApiClient,
    upload_client: &mut UploadClient,
    cycle: &CycleInputs,
    monitoring_enabled: bool,
    config: &BatchDaemonConfig,
    runtime: &mut RuntimeState,
) {
    let attempt_settings_refresh = match runtime.last_settings_attempt {
        Some(last) => last.elapsed() >= config.settings_fetch_retry_interval,
        None => true,
    };
    let needs_settings_refresh = runtime
        .last_settings_fetch
        .map(|t| t.elapsed() >= config.settings_refresh_interval)
        .unwrap_or(true);

    if attempt_settings_refresh && (runtime.device_settings.is_none() || needs_settings_refresh) {
        runtime.last_settings_attempt = Some(Instant::now());
        match api_client
            .get_device(&cycle.access_token, &cycle.device_id)
            .await
        {
            Ok(settings) => {
                runtime.device_settings = Some(settings);
                runtime.last_settings_fetch = Some(Instant::now());
            }
            Err(err) => emit_warn(host, &format!("device settings fetch failed: {err}")),
        }
    }

    if runtime
        .last_hash_server_fetch
        .map(|t| t.elapsed() >= config.settings_refresh_interval)
        .unwrap_or(true)
    {
        match api_client
            .get_hash_server_url(&cycle.access_token, &cycle.device_id)
            .await
        {
            Ok(hash_url) => match UploadClient::with_config(UploadClientConfig {
                hash_base_url: Some(hash_url),
                ..UploadClientConfig::default()
            }) {
                Ok(client) => {
                    *upload_client = client;
                    runtime.last_hash_server_fetch = Some(Instant::now());
                }
                Err(err) => emit_warn(host, &format!("failed to build upload client: {err}")),
            },
            Err(err) => emit_warn(host, &format!("hash server URL fetch failed: {err}")),
        }
    }

    if runtime.device_settings.is_none() {
        runtime.device_settings = Some(Device {
            id: cycle.device_id.clone(),
            enabled: monitoring_enabled,
        });
    }
}

fn capture_enabled(runtime: &RuntimeState, monitoring_enabled: bool) -> bool {
    runtime
        .device_settings
        .as_ref()
        .map(|settings| settings.enabled)
        .unwrap_or(monitoring_enabled)
}

async fn wait_for_next_capture<H: ServiceHost>(
    host: &H,
    runtime: &mut RuntimeState,
) -> CoreResult<bool> {
    let policy = CaptureSchedulePolicy {
        base_interval: Duration::from_secs(resolve_capture_interval_seconds()),
        ..CaptureSchedulePolicy::default()
    };
    let mut rng = thread_rng();
    let delay = policy.next_delay(
        &mut runtime.schedule_state,
        runtime.last_cycle_success,
        &mut rng,
    );
    if sleep_or_stop(host, delay).await? {
        runtime.stopping = true;
        return Ok(true);
    }
    Ok(runtime.stopping || host.should_stop())
}

async fn flush_batch_if_due<H: ServiceHost>(
    host: &H,
    upload_client: &UploadClient,
    cycle: &CycleInputs,
    now: DateTime<Utc>,
    batch_buffer_path: &Path,
    runtime: &mut RuntimeState,
) -> bool {
    let window_age_secs = (now - runtime.batch_window_start).num_seconds().max(0) as u64;
    if window_age_secs < resolve_batch_window_seconds() || runtime.batch_buffer.items.is_empty() {
        return false;
    }

    let end_time = now;
    let start_time = runtime.batch_window_start;

    let items = std::mem::take(&mut runtime.batch_buffer.items);
    let blob = BatchBlob::new(items.clone());

    match upload_client
        .upload_batch(
            &cycle.access_token,
            &cycle.device_id,
            &blob,
            start_time,
            end_time,
            &cycle.e2ee_key,
        )
        .await
    {
        Ok(resp) => {
            emit_info(host, &format!("batch uploaded: {}", resp.batch.id));
            runtime.batch_buffer = BatchBuffer::default();
            runtime.batch_window_start = now;
        }
        Err(err) => {
            emit_warn(host, &format!("batch upload failed: {err}"));
            runtime.batch_buffer.items = items;
        }
    }

    save_batch_buffer(batch_buffer_path, &runtime.batch_buffer);
    true
}

async fn process_capture<H: ServiceHost>(
    ctx: &DaemonContext<'_, H>,
    upload_client: &UploadClient,
    cycle: &CycleInputs,
    now: DateTime<Utc>,
    runtime: &mut RuntimeState,
) -> CoreResult<()> {
    let raw_capture = match ctx.host.capture_frame_png().await {
        Ok(CaptureOutcome::FramePng(bytes)) => bytes,
        Ok(CaptureOutcome::SessionUnavailable) => {
            runtime.last_cycle_success = true;
            let should_log = runtime
                .last_session_unavailable_log
                .map(|when| when.elapsed() >= ctx.config.session_unavailable_log_interval)
                .unwrap_or(true);
            if should_log {
                emit_warn(ctx.host, "session unavailable for capture");
                runtime.last_session_unavailable_log = Some(Instant::now());
            }
            if sleep_or_stop(ctx.host, ctx.config.idle_retry_interval).await? {
                runtime.stopping = true;
            }
            return Ok(());
        }
        Ok(CaptureOutcome::PermissionMissing) => {
            runtime.last_cycle_success = false;
            let item = make_missed_capture(now, "permission_missing", "capture permission missing");
            upload_hash_for_item(
                ctx.host,
                upload_client,
                &cycle.access_token,
                &cycle.device_id,
                &item,
            )
            .await;
            enqueue_batch_item(ctx.batch_buffer_path, runtime, item);
            return Ok(());
        }
        Err(err) => {
            runtime.last_cycle_success = false;
            emit_warn(ctx.host, &format!("capture failed: {err}"));
            let item = make_missed_capture(now, "capture_failed", &err.to_string());
            upload_hash_for_item(
                ctx.host,
                upload_client,
                &cycle.access_token,
                &cycle.device_id,
                &item,
            )
            .await;
            enqueue_batch_item(ctx.batch_buffer_path, runtime, item);
            return Ok(());
        }
    };

    let processed = match ctx.pipeline.process(&raw_capture) {
        Ok(output) => output,
        Err(err) => {
            runtime.last_cycle_success = false;
            emit_warn(ctx.host, &format!("image pipeline failed: {err}"));
            let item = make_missed_capture(now, "image_pipeline_failed", &err.to_string());
            upload_hash_for_item(
                ctx.host,
                upload_client,
                &cycle.access_token,
                &cycle.device_id,
                &item,
            )
            .await;
            enqueue_batch_item(ctx.batch_buffer_path, runtime, item);
            return Ok(());
        }
    };

    let item = BatchItem {
        id: Uuid::new_v4().to_string(),
        taken_at: now.timestamp_millis(),
        kind: "screenshot".to_string(),
        image: Some(processed.bytes),
        metadata: vec![],
    };
    upload_hash_for_item(
        ctx.host,
        upload_client,
        &cycle.access_token,
        &cycle.device_id,
        &item,
    )
    .await;
    enqueue_batch_item(ctx.batch_buffer_path, runtime, item);
    runtime.last_cycle_success = true;
    Ok(())
}

fn enqueue_batch_item(batch_buffer_path: &Path, runtime: &mut RuntimeState, item: BatchItem) {
    runtime.batch_buffer.items.push(item);
    if runtime.batch_buffer.window_start.is_none() {
        runtime.batch_buffer.window_start = Some(runtime.batch_window_start);
    }
    save_batch_buffer(batch_buffer_path, &runtime.batch_buffer);
}

async fn mark_stop_if_sleep_interrupted<H: ServiceHost>(
    host: &H,
    duration: Duration,
    runtime: &mut RuntimeState,
) -> CoreResult<()> {
    if sleep_or_stop(host, duration).await? {
        runtime.stopping = true;
    }
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
