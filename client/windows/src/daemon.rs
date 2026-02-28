use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use rand::thread_rng;
use serde_json::json;
use tokio::time::sleep;
use uuid::Uuid;

use virtue_client_core::{
    AuthClient, BufferedUpload, CaptureSchedulePolicy, CaptureScheduleState, FileTokenStore,
    ImagePipeline, PersistentQueue, RetryPolicy, TokenStore, UploadClient,
    resolve_capture_interval_seconds,
};

use crate::api::ApiClient;
use crate::capture::capture_screen_png;
use crate::config::{ClientPaths, load_state};
use crate::service_log::ServiceLogger;

pub async fn run_daemon(shutdown: Arc<AtomicBool>, logger: &ServiceLogger) -> Result<()> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;

    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let auth_client = AuthClient::new(token_store.clone())?;
    let queue = PersistentQueue::open(&paths.queue_file, 512)?;
    let upload_client = UploadClient::new()?;
    let api_client = ApiClient::new()?;

    let retry_policy = RetryPolicy::default();
    let pipeline = ImagePipeline::default();

    let mut schedule_state = CaptureScheduleState::default();
    let mut last_cycle_success = true;

    logger.info("capture daemon started");

    while !shutdown.load(Ordering::SeqCst) {
        let state = load_state(&paths.state_file)?;
        let Some(mut access_token) = token_store.get_access_token()? else {
            interruptible_sleep(Duration::from_secs(20), &shutdown).await;
            continue;
        };
        let Some(device_id) = state.device_id.clone() else {
            interruptible_sleep(Duration::from_secs(20), &shutdown).await;
            continue;
        };

        if !state.monitoring_enabled {
            interruptible_sleep(Duration::from_secs(20), &shutdown).await;
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
            Err(_) => {
                if interruptible_sleep(Duration::from_secs(20), &shutdown).await {
                    break;
                }
                continue;
            }
        }

        let effective_interval_seconds = resolve_capture_interval_seconds();
        let policy = CaptureSchedulePolicy {
            base_interval: Duration::from_secs(effective_interval_seconds),
            ..CaptureSchedulePolicy::default()
        };

        let mut rng = thread_rng();
        let delay = policy.next_delay(&mut schedule_state, last_cycle_success, &mut rng);
        if interruptible_sleep(delay, &shutdown).await {
            break;
        }

        let raw_capture = match capture_screen_png() {
            Ok(bytes) => bytes,
            Err(err) => {
                last_cycle_success = false;
                logger.warn(&format!("capture failed: {err:#}"));
                let mut metadata = BTreeMap::new();
                metadata.insert("reason".to_string(), json!("capture_failed"));
                metadata.insert("error".to_string(), json!(err.to_string()));
                let _ = api_client
                    .send_log(&access_token, "missed_capture", &device_id, None, metadata)
                    .await;
                continue;
            }
        };

        let processed = match pipeline.process(&raw_capture) {
            Ok(output) => output,
            Err(err) => {
                last_cycle_success = false;
                logger.warn(&format!("image processing failed: {err:#}"));
                let mut metadata = BTreeMap::new();
                metadata.insert("reason".to_string(), json!("image_pipeline_failed"));
                metadata.insert("error".to_string(), json!(err.to_string()));
                let _ = api_client
                    .send_log(&access_token, "missed_capture", &device_id, None, metadata)
                    .await;
                continue;
            }
        };

        let item = BufferedUpload::new(
            Uuid::new_v4().to_string(),
            device_id.clone(),
            Utc::now(),
            processed.content_type,
            processed.bytes,
            processed.sha256_hex,
        );
        let _ = queue.enqueue(item)?;

        match upload_client
            .process_upload_queue(&queue, &retry_policy, &access_token, 8)
            .await
        {
            Ok(report) => {
                last_cycle_success = report.last_error.is_none();
                if report.dropped > 0 {
                    logger.warn(&format!(
                        "upload retries exhausted in core; dropped={} remaining={}",
                        report.dropped, report.remaining
                    ));
                }
            }
            Err(err) => {
                last_cycle_success = false;
                logger.warn(&format!("queue processing failed: {err:#}"));
                let mut metadata = BTreeMap::new();
                metadata.insert("reason".to_string(), json!("queue_processing_failed"));
                metadata.insert("error".to_string(), json!(err.to_string()));
                let _ = api_client
                    .send_log(&access_token, "missed_capture", &device_id, None, metadata)
                    .await;
            }
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
