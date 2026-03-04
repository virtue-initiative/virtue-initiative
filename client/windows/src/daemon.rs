use std::fs;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Utc};
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use uuid::Uuid;
use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, HANDLE};
use windows::Win32::System::Threading::{MUTEX_MODIFY_STATE, OpenMutexW};
use windows::core::w;

use virtue_client_core::{
    ApiClient, AuthClient, BatchBlob, BatchItem, CaptureSchedulePolicy, CaptureScheduleState,
    Device, FileTokenStore, ImagePipeline, TokenStore, UploadClient, UploadClientConfig,
    resolve_batch_window_seconds, resolve_capture_interval_seconds, uuid_str_to_bytes,
};

use crate::capture::capture_screen_png;
use crate::config::{ClientPaths, load_state};
use crate::service_log::ServiceLogger;

const SETTINGS_REFRESH_INTERVAL: Duration = Duration::from_secs(30 * 60);
const SETTINGS_FETCH_RETRY_INTERVAL: Duration = Duration::from_secs(30);
const IDLE_RETRY_INTERVAL: Duration = Duration::from_secs(30);
const TRAY_ENSURE_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Default, Serialize, Deserialize)]
struct BatchBuffer {
    items: Vec<BatchItem>,
    window_start: Option<DateTime<Utc>>,
}

fn load_batch_buffer(path: &std::path::Path) -> BatchBuffer {
    fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_batch_buffer(path: &std::path::Path, buf: &BatchBuffer) {
    if let Ok(bytes) = serde_json::to_vec(buf) {
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, bytes).is_ok() {
            let _ = fs::rename(tmp, path);
        }
    }
}

pub async fn run_daemon(shutdown: Arc<AtomicBool>, logger: &ServiceLogger) -> Result<()> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;

    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let auth_client = AuthClient::new(token_store.clone())?;
    let api_client = ApiClient::new()?;
    let pipeline = ImagePipeline::default();

    let mut schedule_state = CaptureScheduleState::default();
    let mut last_cycle_success = true;
    let mut warned_missing_e2ee = false;
    let mut device_settings: Option<Device> = None;
    let mut last_settings_fetch: Option<Instant> = None;
    let mut last_settings_attempt: Option<Instant> = None;
    let mut last_hash_server_fetch: Option<Instant> = None;
    let mut batch_buffer = load_batch_buffer(&paths.batch_buffer_file);
    let mut batch_window_start: DateTime<Utc> = batch_buffer.window_start.unwrap_or_else(Utc::now);
    let mut last_tray_ensure: Option<Instant> = None;

    logger.info("capture daemon started");

    // Re-fetch the E2EE key from the server on each daemon startup.
    if let Some(access_token) = token_store.get_access_token().ok().flatten() {
        if let Err(err) = auth_client.fetch_and_decrypt_e2ee_key(&access_token).await {
            logger.warn(&format!("could not fetch E2EE key on startup: {err:#}"));
        }
    }

    let mut upload_client = UploadClient::new()?;

    while !shutdown.load(Ordering::SeqCst) {
        let state = match load_state(&paths.state_file) {
            Ok(state) => state,
            Err(err) => {
                logger.warn(&format!("state read failed: {err:#}"));
                if interruptible_sleep(IDLE_RETRY_INTERVAL, &shutdown).await {
                    break;
                }
                continue;
            }
        };

        let Some(mut access_token) = (match token_store.get_access_token() {
            Ok(token) => token,
            Err(err) => {
                logger.warn(&format!("token read failed: {err:#}"));
                if interruptible_sleep(IDLE_RETRY_INTERVAL, &shutdown).await {
                    break;
                }
                continue;
            }
        }) else {
            if interruptible_sleep(IDLE_RETRY_INTERVAL, &shutdown).await {
                break;
            }
            continue;
        };
        let Some(device_id) = state.device_id.clone() else {
            if interruptible_sleep(IDLE_RETRY_INTERVAL, &shutdown).await {
                break;
            }
            continue;
        };
        let Some(e2ee_key) = (match token_store.get_e2ee_key() {
            Ok(key) => key,
            Err(err) => {
                logger.warn(&format!("e2ee key read failed: {err:#}"));
                if interruptible_sleep(IDLE_RETRY_INTERVAL, &shutdown).await {
                    break;
                }
                continue;
            }
        }) else {
            if !warned_missing_e2ee {
                logger.warn("E2EE key not set; sign in again to derive and store it");
                warned_missing_e2ee = true;
            }
            if interruptible_sleep(IDLE_RETRY_INTERVAL, &shutdown).await {
                break;
            }
            continue;
        };
        warned_missing_e2ee = false;

        if !state.monitoring_enabled {
            if interruptible_sleep(IDLE_RETRY_INTERVAL, &shutdown).await {
                break;
            }
            continue;
        }

        let should_ensure_tray = last_tray_ensure
            .map(|t| t.elapsed() >= TRAY_ENSURE_INTERVAL)
            .unwrap_or(true);
        if should_ensure_tray {
            ensure_tray_running(logger);
            last_tray_ensure = Some(Instant::now());
        }

        match auth_client
            .refresh_access_token_if_needed(Duration::from_secs(120))
            .await
        {
            Ok(Some(refreshed)) => {
                access_token = refreshed.access_token;
            }
            Ok(None) => {}
            Err(err) => {
                logger.warn(&format!("token refresh failed: {err}"));
                // Keep capturing even if the API is temporarily unreachable.
            }
        }

        let attempt_settings_refresh = match last_settings_attempt {
            Some(last) => last.elapsed() >= SETTINGS_FETCH_RETRY_INTERVAL,
            None => true,
        };
        let needs_settings_refresh = last_settings_fetch
            .map(|t| t.elapsed() >= SETTINGS_REFRESH_INTERVAL)
            .unwrap_or(true);

        if attempt_settings_refresh && (device_settings.is_none() || needs_settings_refresh) {
            last_settings_attempt = Some(Instant::now());
            match api_client.get_device(&access_token, &device_id).await {
                Ok(settings) => {
                    device_settings = Some(settings);
                    last_settings_fetch = Some(Instant::now());
                }
                Err(err) => {
                    logger.warn(&format!("device settings fetch failed: {err}"));
                }
            }
        }

        if last_hash_server_fetch
            .map(|t| t.elapsed() >= SETTINGS_REFRESH_INTERVAL)
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
                        Err(err) => logger.warn(&format!("failed to build upload client: {err}")),
                    }
                }
                Err(err) => logger.warn(&format!("hash server URL fetch failed: {err}")),
            }
        }

        let capture_enabled = device_settings
            .as_ref()
            .map(|settings| settings.enabled)
            .unwrap_or(state.monitoring_enabled);

        if !capture_enabled {
            if interruptible_sleep(IDLE_RETRY_INTERVAL, &shutdown).await {
                break;
            }
            continue;
        }

        let policy = CaptureSchedulePolicy {
            base_interval: Duration::from_secs(resolve_capture_interval_seconds()),
            ..CaptureSchedulePolicy::default()
        };

        let mut rng = thread_rng();
        let delay = policy.next_delay(&mut schedule_state, last_cycle_success, &mut rng);
        if interruptible_sleep(delay, &shutdown).await {
            break;
        }
        let now = Utc::now();

        let window_age_secs = (now - batch_window_start).num_seconds() as u64;
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
                    logger.info(&format!("batch uploaded: {}", resp.batch.id));
                    batch_buffer = BatchBuffer::default();
                    batch_window_start = now;
                }
                Err(err) => {
                    logger.warn(&format!("batch upload failed: {err}"));
                    batch_buffer.items = items;
                }
            }

            save_batch_buffer(&paths.batch_buffer_file, &batch_buffer);
            continue;
        }

        let raw_capture = match capture_screen_png() {
            Ok(bytes) => bytes,
            Err(err) => {
                last_cycle_success = false;
                logger.warn(&format!("capture failed: {err}"));
                let item = BatchItem {
                    id: Uuid::new_v4().to_string(),
                    taken_at: now.timestamp_millis(),
                    kind: "missed_capture".to_string(),
                    image: None,
                    metadata: vec![
                        ("reason".to_string(), "capture_failed".to_string()),
                        ("error".to_string(), err.to_string()),
                    ],
                };
                if let Some(device_id_bytes) = uuid_str_to_bytes(&device_id) {
                    if let Err(e) = upload_client
                        .upload_hash(&access_token, &device_id_bytes, &item.sha256())
                        .await
                    {
                        logger.warn(&format!("content hash upload failed: {e}"));
                    }
                }
                batch_buffer.items.push(item);
                if batch_buffer.window_start.is_none() {
                    batch_buffer.window_start = Some(batch_window_start);
                }
                save_batch_buffer(&paths.batch_buffer_file, &batch_buffer);
                continue;
            }
        };

        let processed = match pipeline.process(&raw_capture) {
            Ok(output) => output,
            Err(err) => {
                last_cycle_success = false;
                logger.warn(&format!("image pipeline failed: {err}"));
                let item = BatchItem {
                    id: Uuid::new_v4().to_string(),
                    taken_at: now.timestamp_millis(),
                    kind: "missed_capture".to_string(),
                    image: None,
                    metadata: vec![
                        ("reason".to_string(), "image_pipeline_failed".to_string()),
                        ("error".to_string(), err.to_string()),
                    ],
                };
                if let Some(device_id_bytes) = uuid_str_to_bytes(&device_id) {
                    if let Err(e) = upload_client
                        .upload_hash(&access_token, &device_id_bytes, &item.sha256())
                        .await
                    {
                        logger.warn(&format!("content hash upload failed: {e}"));
                    }
                }
                batch_buffer.items.push(item);
                save_batch_buffer(&paths.batch_buffer_file, &batch_buffer);
                continue;
            }
        };

        // Build the item first so the hash covers all fields.
        let item = BatchItem {
            id: Uuid::new_v4().to_string(),
            taken_at: now.timestamp_millis(),
            kind: "screenshot".to_string(),
            image: Some(processed.bytes),
            metadata: vec![],
        };

        // Upload the content hash (covers all item fields) immediately after capture.
        if let Some(device_id_bytes) = uuid_str_to_bytes(&device_id) {
            let content_hash = item.sha256();
            if let Err(err) = upload_client
                .upload_hash(&access_token, &device_id_bytes, &content_hash)
                .await
            {
                logger.warn(&format!("content hash upload failed: {err}"));
            }
        }

        batch_buffer.items.push(item);
        if batch_buffer.window_start.is_none() {
            batch_buffer.window_start = Some(batch_window_start);
        }
        save_batch_buffer(&paths.batch_buffer_file, &batch_buffer);
        last_cycle_success = true;
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
    }

    logger.info("capture daemon stopping");
    Ok(())
}

async fn interruptible_sleep(duration: Duration, shutdown: &Arc<AtomicBool>) -> bool {
    let mut remaining = duration;

    while remaining > Duration::ZERO {
        if shutdown.load(Ordering::SeqCst) {
            return true;
        }

        let tick = remaining.min(Duration::from_secs(1));
        sleep(tick).await;
        remaining = remaining.saturating_sub(tick);
    }

    shutdown.load(Ordering::SeqCst)
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
    // Use the tray mutex as the canonical process-liveness signal.
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
