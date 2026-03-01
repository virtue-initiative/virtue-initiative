use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Utc};
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use uuid::Uuid;

use virtue_client_core::{
    AuthClient, BatchBlob, BatchItem, CaptureSchedulePolicy, CaptureScheduleState, ChainHasher,
    FileTokenStore, ImagePipeline, TokenStore, UploadClient, resolve_batch_window_seconds,
    resolve_capture_interval_seconds, sha256_bytes,
};

use crate::api::{ApiClient, Device};
use crate::capture::capture_screen_png;
use crate::config::{ClientPaths, load_state};
use crate::service_log::ServiceLogger;

const SETTINGS_REFRESH_INTERVAL: Duration = Duration::from_secs(30 * 60);
const IDLE_RETRY_INTERVAL: Duration = Duration::from_secs(20);
const HASH_INTERVAL_SECONDS: u64 = 60;

#[derive(Debug, Default, Serialize, Deserialize)]
struct BatchBuffer {
    items: Vec<BatchItem>,
    window_start: Option<DateTime<Utc>>,
    start_chain_hash_hex: Option<String>,
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
    let upload_client = UploadClient::new()?;
    let api_client = ApiClient::new()?;
    let pipeline = ImagePipeline::default();

    let mut schedule_state = CaptureScheduleState::default();
    let mut last_cycle_success = true;
    let mut warned_missing_e2ee = false;
    let mut device_settings: Option<Device> = None;
    let mut last_settings_fetch: Option<Instant> = None;
    let mut chain_hasher = ChainHasher::new();
    let mut last_hash_sent: Option<DateTime<Utc>> = None;
    let mut last_image_sha256: Option<[u8; 32]> = None;
    let mut batch_buffer = load_batch_buffer(&paths.batch_buffer_file);
    let mut batch_window_start: DateTime<Utc> = batch_buffer.window_start.unwrap_or_else(Utc::now);

    logger.info("capture daemon started");

    while !shutdown.load(Ordering::SeqCst) {
        let state = load_state(&paths.state_file)?;
        let Some(mut access_token) = token_store.get_access_token()? else {
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
        let Some(e2ee_key) = token_store.get_e2ee_key()? else {
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
                if interruptible_sleep(IDLE_RETRY_INTERVAL, &shutdown).await {
                    break;
                }
                continue;
            }
        }

        let needs_refresh = last_settings_fetch
            .map(|t| t.elapsed() >= SETTINGS_REFRESH_INTERVAL)
            .unwrap_or(true);
        if needs_refresh {
            match api_client.get_device(&access_token, &device_id).await {
                Ok(settings) => {
                    device_settings = Some(settings);
                    last_settings_fetch = Some(Instant::now());
                }
                Err(err) => {
                    logger.warn(&format!("device settings fetch failed: {err}"));
                    if interruptible_sleep(IDLE_RETRY_INTERVAL, &shutdown).await {
                        break;
                    }
                    continue;
                }
            }
        }

        let settings = match device_settings.as_ref() {
            Some(settings) => settings,
            None => {
                if interruptible_sleep(IDLE_RETRY_INTERVAL, &shutdown).await {
                    break;
                }
                continue;
            }
        };

        if !settings.enabled {
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

        let should_send_hash = last_hash_sent
            .map(|t| (now - t).num_seconds() >= HASH_INTERVAL_SECONDS as i64)
            .unwrap_or(true);

        if should_send_hash {
            let unix_minute = now.timestamp() as u64 / HASH_INTERVAL_SECONDS;
            let hash = chain_hasher.next(last_image_sha256.as_ref(), unix_minute);
            last_image_sha256 = None;

            if batch_buffer.start_chain_hash_hex.is_none() {
                batch_buffer.start_chain_hash_hex = Some(hex::encode(chain_hasher.start_hash()));
            }

            if let Err(err) = upload_client
                .upload_hash(&access_token, &device_id, &hash, now)
                .await
            {
                logger.warn(&format!("chain hash upload failed: {err}"));
            }
            last_hash_sent = Some(now);
        }

        let window_age_secs = (now - batch_window_start).num_seconds() as u64;
        if window_age_secs >= resolve_batch_window_seconds() && !batch_buffer.items.is_empty() {
            let end_time = now;
            let start_time = batch_window_start;

            let start_hash = hex::decode(batch_buffer.start_chain_hash_hex.as_deref().unwrap_or(""))
                .unwrap_or_else(|_| vec![0u8; 32]);
            let start_hash_arr: [u8; 32] = start_hash.try_into().unwrap_or([0u8; 32]);
            let end_hash_arr = chain_hasher.latest_hash();

            let items = std::mem::take(&mut batch_buffer.items);
            let blob = BatchBlob::new(items.clone());

            match upload_client
                .upload_batch(
                    &access_token,
                    &device_id,
                    &blob,
                    start_time,
                    end_time,
                    &start_hash_arr,
                    &end_hash_arr,
                    &e2ee_key,
                )
                .await
            {
                Ok(resp) => {
                    logger.info(&format!("batch uploaded: {}", resp.batch.id));
                    chain_hasher.reset_for_new_batch();
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
                batch_buffer.items.push(item);
                save_batch_buffer(&paths.batch_buffer_file, &batch_buffer);
                continue;
            }
        };

        last_image_sha256 = Some(sha256_bytes(&processed.bytes));

        let item = BatchItem {
            id: Uuid::new_v4().to_string(),
            taken_at: now.timestamp_millis(),
            kind: "screenshot".to_string(),
            image: Some(processed.bytes),
            metadata: vec![],
        };
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
