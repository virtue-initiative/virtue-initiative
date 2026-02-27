use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Utc};
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use uuid::Uuid;

use bepure_client_core::{
    AuthClient, BatchBlob, BatchItem, CaptureSchedulePolicy, CaptureScheduleState,
    ChainHasher, DEFAULT_CAPTURE_INTERVAL_SECONDS, FileTokenStore, ImagePipeline,
    TokenStore, UploadClient, sha256_bytes,
};

use crate::api::{ApiClient, Device};
use crate::capture::{capture_screen, is_session_unavailable_error};
use crate::config::{ClientPaths, load_state};

const SETTINGS_REFRESH_INTERVAL: Duration = Duration::from_secs(30 * 60);
const IDLE_RETRY_INTERVAL: Duration = Duration::from_secs(20);
const SESSION_UNAVAILABLE_LOG_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Send a chain hash every minute.
const HASH_INTERVAL_SECONDS: u64 = 60;

/// Persisted batch buffer — the current hour's items, saved to disk across restarts.
#[derive(Debug, Default, Serialize, Deserialize)]
struct BatchBuffer {
    items: Vec<BatchItem>,
    window_start: Option<DateTime<Utc>>,
    /// Hex of the first chain hash in this window.
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

pub async fn run_daemon(paths: &ClientPaths) -> Result<()> {
    paths.ensure_dirs()?;

    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let auth_client = AuthClient::new(token_store.clone())?;
    let upload_client = UploadClient::new()?;
    let api_client = ApiClient::new()?;
    let pipeline = ImagePipeline::default();

    let mut schedule_state = CaptureScheduleState::default();
    let mut last_cycle_success = true;
    let mut device_settings: Option<Device> = None;
    let mut last_settings_fetch: Option<Instant> = None;
    let mut last_session_unavailable_log: Option<Instant> = None;

    let mut chain_hasher = ChainHasher::new();
    let mut last_hash_sent: Option<DateTime<Utc>> = None;
    let mut last_image_sha256: Option<[u8; 32]> = None;
    let mut batch_buffer = load_batch_buffer(&paths.batch_buffer_file);
    let mut batch_window_start: DateTime<Utc> = batch_buffer
        .window_start
        .unwrap_or_else(Utc::now);

    loop {
        let state = load_state(&paths.state_file)?;
        let Some(mut access_token) = token_store.get_access_token()? else {
            sleep(IDLE_RETRY_INTERVAL).await;
            continue;
        };
        let Some(device_id) = state.device_id.clone() else {
            sleep(IDLE_RETRY_INTERVAL).await;
            continue;
        };
        let Some(e2ee_key) = token_store.get_e2ee_key()? else {
            eprintln!("daemon: E2EE key not set — run `bepure login` again");
            sleep(IDLE_RETRY_INTERVAL).await;
            continue;
        };

        if !state.monitoring_enabled {
            sleep(IDLE_RETRY_INTERVAL).await;
            continue;
        }

        match auth_client
            .refresh_access_token_if_needed(Duration::from_secs(120))
            .await
        {
            Ok(Some(refreshed)) => access_token = refreshed.access_token,
            Ok(None) => {}
            Err(_) => {
                sleep(IDLE_RETRY_INTERVAL).await;
                continue;
            }
        }

        // Fetch device settings periodically.
        let needs_refresh = last_settings_fetch
            .map(|t| t.elapsed() >= SETTINGS_REFRESH_INTERVAL)
            .unwrap_or(true);
        if needs_refresh {
            match api_client.get_device(&access_token, &device_id).await {
                Ok(settings) => {
                    device_settings = Some(settings);
                    last_settings_fetch = Some(Instant::now());
                }
                Err(_) => {
                    sleep(IDLE_RETRY_INTERVAL).await;
                    continue;
                }
            }
        }

        let settings = device_settings.as_ref().unwrap();
        if !settings.enabled {
            sleep(IDLE_RETRY_INTERVAL).await;
            continue;
        }

        let interval_seconds = if settings.interval_seconds > 0 {
            settings.interval_seconds
        } else {
            DEFAULT_CAPTURE_INTERVAL_SECONDS
        };

        let policy = CaptureSchedulePolicy {
            base_interval: Duration::from_secs(interval_seconds),
            ..CaptureSchedulePolicy::default()
        };
        let mut rng = thread_rng();
        let delay = policy.next_delay(&mut schedule_state, last_cycle_success, &mut rng);
        sleep(delay).await;

        let now = Utc::now();

        // ── Per-minute hash chain ──────────────────────────────────────────────
        let should_send_hash = last_hash_sent
            .map(|t| (now - t).num_seconds() >= HASH_INTERVAL_SECONDS as i64)
            .unwrap_or(true);

        if should_send_hash {
            let unix_minute = now.timestamp() as u64 / HASH_INTERVAL_SECONDS;
            let hash = chain_hasher.next(last_image_sha256.as_ref(), unix_minute);
            last_image_sha256 = None; // consume — used once per minute

            // Record the start hash for this batch window.
            if batch_buffer.start_chain_hash_hex.is_none() {
                batch_buffer.start_chain_hash_hex = Some(hex::encode(chain_hasher.start_hash()));
            }

            if let Err(e) = upload_client
                .upload_hash(&access_token, &device_id, &hash, now)
                .await
            {
                eprintln!("failed to upload chain hash: {e}");
            }
            last_hash_sent = Some(now);
        }

        // ── Batch flush ────────────────────────────────────────────────────────
        let window_age_secs = (now - batch_window_start).num_seconds() as u64;
        if window_age_secs >= state.batch_window_seconds && !batch_buffer.items.is_empty() {
            let end_time = now;
            let start_time = batch_window_start;

            let start_hash = hex::decode(
                batch_buffer.start_chain_hash_hex.as_deref().unwrap_or(""),
            )
            .unwrap_or_else(|_| vec![0u8; 32]);
            let start_hash_arr: [u8; 32] = start_hash.try_into().unwrap_or([0u8; 32]);
            let end_hash_arr = chain_hasher.latest_hash();

            let mut items = std::mem::take(&mut batch_buffer.items);
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
                    eprintln!("batch uploaded: {}", resp.batch.id);
                    chain_hasher.reset_for_new_batch();
                    batch_buffer = BatchBuffer::default();
                    batch_window_start = now;
                }
                Err(e) => {
                    eprintln!("batch upload failed: {e}");
                    // restore items so they aren't lost
                    batch_buffer.items = items;
                }
            }

            save_batch_buffer(&paths.batch_buffer_file, &batch_buffer);
            continue;
        }

        // ── Screen capture ─────────────────────────────────────────────────────
        let raw_capture = match capture_screen(state.backend_hint) {
            Ok(bytes) => bytes,
            Err(err) => {
                if is_session_unavailable_error(&err) {
                    last_cycle_success = true;
                    let should_log = last_session_unavailable_log
                        .map(|when| when.elapsed() >= SESSION_UNAVAILABLE_LOG_INTERVAL)
                        .unwrap_or(true);
                    if should_log {
                        eprintln!("session unavailable for capture: {err}");
                        last_session_unavailable_log = Some(Instant::now());
                    }
                    sleep(IDLE_RETRY_INTERVAL).await;
                    continue;
                }
                last_cycle_success = false;
                eprintln!("capture failed: {err}");
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
                eprintln!("image pipeline failed: {err}");
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

        // Store the SHA-256 for the hash chain (consumed by next hash tick).
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
    }
}
