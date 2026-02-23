use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use rand::thread_rng;
use serde_json::json;
use tokio::time::sleep;
use uuid::Uuid;

use bepure_client_core::{
    AuthClient, BufferedUpload, CaptureSchedulePolicy, CaptureScheduleState, FileTokenStore,
    ImagePipeline, PersistentQueue, RetryPolicy, TokenStore, UploadClient,
    resolve_capture_interval_seconds,
};

use crate::api::ApiClient;
use crate::capture::{capture_screen, has_screen_capture_access, request_screen_capture_access};
use crate::config::{
    ClientPaths, ScreenshotPermissionStatus, load_daemon_status, load_state, save_daemon_status,
};

pub async fn run_daemon(paths: &ClientPaths) -> Result<()> {
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
    let mut permission_prompt_requested = false;

    loop {
        let state = load_state(&paths.state_file)?;
        let Some(mut access_token) = token_store.get_access_token()? else {
            sleep(Duration::from_secs(20)).await;
            continue;
        };
        let Some(device_id) = state.device_id.clone() else {
            sleep(Duration::from_secs(20)).await;
            continue;
        };

        if !state.monitoring_enabled {
            sleep(Duration::from_secs(20)).await;
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
                sleep(Duration::from_secs(20)).await;
                continue;
            }
        }

        let effective_interval_seconds =
            resolve_capture_interval_seconds(state.capture_interval_seconds);
        let policy = CaptureSchedulePolicy {
            base_interval: Duration::from_secs(effective_interval_seconds),
            ..CaptureSchedulePolicy::default()
        };
        let mut rng = thread_rng();
        let delay = policy.next_delay(&mut schedule_state, last_cycle_success, &mut rng);
        sleep(delay).await;

        if !has_screen_capture_access() {
            last_cycle_success = false;
            if !permission_prompt_requested {
                let _ = request_screen_capture_access();
                permission_prompt_requested = true;
            }
            let error_text = "screen recording permission missing for daemon process".to_string();
            update_daemon_status(
                paths,
                ScreenshotPermissionStatus::Missing,
                Some(error_text.clone()),
            );
            let mut metadata = BTreeMap::new();
            metadata.insert("reason".to_string(), json!("permission_missing"));
            metadata.insert("error".to_string(), json!(error_text));
            let _ = api_client
                .send_log(&access_token, "missed_capture", &device_id, None, metadata)
                .await;
            continue;
        }
        permission_prompt_requested = false;

        let raw_capture = match capture_screen() {
            Ok(bytes) => bytes,
            Err(err) => {
                last_cycle_success = false;
                let error_text = format!("{err:#}");
                let screenshot_permission = if is_permission_missing_error(&error_text) {
                    ScreenshotPermissionStatus::Missing
                } else {
                    ScreenshotPermissionStatus::Unknown
                };
                update_daemon_status(paths, screenshot_permission, Some(error_text.clone()));
                let mut metadata = BTreeMap::new();
                metadata.insert("reason".to_string(), json!("capture_failed"));
                metadata.insert("error".to_string(), json!(error_text));
                let _ = api_client
                    .send_log(&access_token, "missed_capture", &device_id, None, metadata)
                    .await;
                continue;
            }
        };
        update_daemon_status(paths, ScreenshotPermissionStatus::Granted, None);

        let processed = match pipeline.process(&raw_capture) {
            Ok(output) => output,
            Err(err) => {
                last_cycle_success = false;
                let mut metadata = BTreeMap::new();
                metadata.insert("reason".to_string(), json!("image_pipeline_failed"));
                metadata.insert("error".to_string(), json!(format!("{err:#}")));
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
            }
            Err(err) => {
                last_cycle_success = false;
                let mut metadata = BTreeMap::new();
                metadata.insert("reason".to_string(), json!("queue_processing_failed"));
                metadata.insert("error".to_string(), json!(format!("{err:#}")));
                let _ = api_client
                    .send_log(&access_token, "missed_capture", &device_id, None, metadata)
                    .await;
            }
        }
    }
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
