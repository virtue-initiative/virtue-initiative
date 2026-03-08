use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::{TimeDelta, Utc};
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jboolean, jlong, jstring};
use jni::JNIEnv;
use once_cell::sync::OnceCell;
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;
use uuid::Uuid;

use virtue_client_core::batch::BatchValue;
use virtue_client_core::queue::{BufferedUpload, PersistentQueue};
use virtue_client_core::{
    apply_dev_env, login_and_register_device, logout_and_clear_tokens_with_alert,
    resolve_batch_window_seconds, resolve_capture_interval_seconds, ApiClient, AuthClient,
    BatchBlob, BatchItem, CaptureSchedulePolicy, CaptureScheduleState, FileTokenStore,
    ImagePipeline, LoginCommandInput, RetryPolicy, TokenStore, UploadClient, BASE_API_URL_ENV_VAR,
    BATCH_WINDOW_SECONDS_ENV_VAR, CAPTURE_INTERVAL_SECONDS_ENV_VAR,
};

static CORE: OnceCell<AndroidCore> = OnceCell::new();

struct AndroidCore {
    runtime: Runtime,
    token_store: Arc<dyn TokenStore>,
    queue: PersistentQueue,
    pipeline: ImagePipeline,
    retry_policy: RetryPolicy,
    schedule_state: Mutex<CaptureScheduleState>,
    state_path: PathBuf,
    dynamic: Mutex<DynamicCore>,
}

#[derive(Clone)]
struct DynamicCore {
    auth_client: AuthClient,
    api_client: ApiClient,
    upload_client: UploadClient,
    schedule_policy: CaptureSchedulePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AndroidState {
    device_id: Option<String>,
}

#[no_mangle]
pub extern "system" fn Java_codes_anb_virtue_NativeBridge_nativeInit(
    mut env: JNIEnv,
    _class: JClass,
    config_dir: JString,
    data_dir: JString,
    base_api_url: JString,
    capture_interval_seconds: JString,
    batch_window_seconds: JString,
) -> jstring {
    let result = (|| -> Result<()> {
        let config_dir: String = env.get_string(&config_dir)?.into();
        let data_dir: String = env.get_string(&data_dir)?.into();
        let base_api_url: String = env.get_string(&base_api_url)?.into();
        let capture_interval_seconds: String = env.get_string(&capture_interval_seconds)?.into();
        let batch_window_seconds: String = env.get_string(&batch_window_seconds)?.into();

        apply_dev_env();
        apply_overrides(
            &base_api_url,
            &capture_interval_seconds,
            &batch_window_seconds,
        );

        if let Some(core) = CORE.get() {
            refresh_dynamic(core)?;
            return Ok(());
        }

        fs::create_dir_all(&config_dir)
            .with_context(|| format!("failed to create config dir {config_dir}"))?;
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("failed to create data dir {data_dir}"))?;

        let token_file = Path::new(&config_dir).join("token_store.json");
        let queue_file = Path::new(&data_dir).join("upload_queue.json");
        let state_file = Path::new(&config_dir).join("android_client_state.json");

        let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&token_file));
        let queue = PersistentQueue::open(&queue_file, 512)?;
        let runtime = Runtime::new().context("failed to build tokio runtime")?;
        let dynamic = build_dynamic(token_store.clone())?;

        CORE.set(AndroidCore {
            runtime,
            token_store,
            queue,
            pipeline: ImagePipeline,
            retry_policy: RetryPolicy::default(),
            schedule_state: Mutex::new(CaptureScheduleState::default()),
            state_path: state_file,
            dynamic: Mutex::new(dynamic),
        })
        .map_err(|_| anyhow!("core already initialized"))?;

        Ok(())
    })();

    to_jstring_result(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_codes_anb_virtue_NativeBridge_nativeSetOverrides(
    mut env: JNIEnv,
    _class: JClass,
    base_api_url: JString,
    capture_interval_seconds: JString,
    batch_window_seconds: JString,
) -> jstring {
    let result = (|| -> Result<()> {
        let base_api_url: String = env.get_string(&base_api_url)?.into();
        let capture_interval_seconds: String = env.get_string(&capture_interval_seconds)?.into();
        let batch_window_seconds: String = env.get_string(&batch_window_seconds)?.into();

        apply_overrides(
            &base_api_url,
            &capture_interval_seconds,
            &batch_window_seconds,
        );

        let core = core()?;
        refresh_dynamic(core)?;
        Ok(())
    })();

    to_jstring_result(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_codes_anb_virtue_NativeBridge_nativeLogin(
    mut env: JNIEnv,
    _class: JClass,
    email: JString,
    password: JString,
    device_name: JString,
) -> jstring {
    let result = (|| -> Result<()> {
        let core = core()?;
        let email: String = env.get_string(&email)?.into();
        let password: String = env.get_string(&password)?.into();
        let device_name: String = env.get_string(&device_name)?.into();

        let (auth_client, api_client) = {
            let dynamic = core
                .dynamic
                .lock()
                .map_err(|_| anyhow!("dynamic core lock poisoned"))?;
            (dynamic.auth_client.clone(), dynamic.api_client.clone())
        };

        core.runtime.block_on(async {
            let login = login_and_register_device(
                &auth_client,
                &api_client,
                core.token_store.as_ref(),
                LoginCommandInput {
                    email: &email,
                    password: &password,
                    device_name: &device_name,
                    platform: "android",
                },
            )
            .await?;

            let mut state = load_state(&core.state_path)?;
            state.device_id = Some(login.device_id);
            save_state(&core.state_path, &state)?;
            Ok::<(), anyhow::Error>(())
        })
    })();

    to_jstring_result(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_codes_anb_virtue_NativeBridge_nativeLogout(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let result = (|| -> Result<()> {
        let core = core()?;

        let (auth_client, api_client) = {
            let dynamic = core
                .dynamic
                .lock()
                .map_err(|_| anyhow!("dynamic core lock poisoned"))?;
            (dynamic.auth_client.clone(), dynamic.api_client.clone())
        };

        core.runtime.block_on(async {
            let mut state = load_state(&core.state_path)?;
            let metadata = vec![
                ("source".to_string(), "android".to_string()),
                ("reason".to_string(), "user_logout".to_string()),
            ];

            let _ = logout_and_clear_tokens_with_alert(
                &auth_client,
                Some(&api_client),
                core.token_store.as_ref(),
                state.device_id.as_deref(),
                &metadata,
            )
            .await;

            state.device_id = None;
            save_state(&core.state_path, &state)?;
            Ok::<(), anyhow::Error>(())
        })
    })();

    to_jstring_result(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_codes_anb_virtue_NativeBridge_nativeIsLoggedIn(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    let result = (|| -> Result<bool> {
        let core = core()?;
        let token = core.token_store.get_access_token()?;
        let state = load_state(&core.state_path)?;
        Ok(token.is_some() && state.device_id.is_some())
    })();

    match result {
        Ok(true) => 1,
        _ => 0,
    }
}

#[no_mangle]
pub extern "system" fn Java_codes_anb_virtue_NativeBridge_nativeGetDeviceId(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let result = (|| -> Result<Option<String>> {
        let core = core()?;
        let state = load_state(&core.state_path)?;
        Ok(state.device_id)
    })();

    match result {
        Ok(Some(device_id)) => to_jstring(&mut env, &device_id),
        _ => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_codes_anb_virtue_NativeBridge_nativeNextCaptureDelayMs(
    _env: JNIEnv,
    _class: JClass,
    last_success: jboolean,
) -> jlong {
    let result = (|| -> Result<i64> {
        let core = core()?;
        let schedule_policy = {
            let dynamic = core
                .dynamic
                .lock()
                .map_err(|_| anyhow!("dynamic core lock poisoned"))?;
            dynamic.schedule_policy.clone()
        };

        let mut state = core
            .schedule_state
            .lock()
            .map_err(|_| anyhow!("schedule state lock poisoned"))?;
        let mut rng = thread_rng();

        let delay = schedule_policy.next_delay(&mut state, last_success != 0, &mut rng);
        Ok(delay.as_millis().min(i64::MAX as u128) as i64)
    })();

    result.unwrap_or(30_000)
}

#[no_mangle]
pub extern "system" fn Java_codes_anb_virtue_NativeBridge_nativeProcessCapture(
    mut env: JNIEnv,
    _class: JClass,
    png_bytes: JByteArray,
) -> jstring {
    let result = (|| -> Result<()> {
        let core = core()?;
        let bytes = env.convert_byte_array(&png_bytes)?;

        let upload_client = {
            let dynamic = core
                .dynamic
                .lock()
                .map_err(|_| anyhow!("dynamic core lock poisoned"))?;
            dynamic.upload_client.clone()
        };

        core.runtime.block_on(async {
            let access_token = core
                .token_store
                .get_access_token()?
                .ok_or_else(|| anyhow!("not logged in"))?;
            let state = load_state(&core.state_path)?;
            let device_id = state
                .device_id
                .ok_or_else(|| anyhow!("device id missing"))?;

            let processed = core.pipeline.process(&bytes)?;

            let item = BufferedUpload::new(
                Uuid::new_v4().to_string(),
                device_id.clone(),
                Utc::now(),
                processed.content_type,
                processed.bytes,
                processed.sha256_hex,
            );
            core.queue.enqueue(item)?;

            process_upload_queue(core, &upload_client, &access_token, &device_id, 12).await
        })
    })();

    to_jstring_result(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_codes_anb_virtue_NativeBridge_nativeReportLog(
    mut env: JNIEnv,
    _class: JClass,
    event_type: JString,
    reason: JString,
    detail: JString,
) -> jstring {
    let result = (|| -> Result<()> {
        let core = core()?;

        let event_type: String = env.get_string(&event_type)?.into();
        let reason: String = env.get_string(&reason)?.into();
        let detail = match env.get_string(&detail) {
            Ok(value) => Some(String::from(value)),
            Err(_) => None,
        };

        let api_client = {
            let dynamic = core
                .dynamic
                .lock()
                .map_err(|_| anyhow!("dynamic core lock poisoned"))?;
            dynamic.api_client.clone()
        };

        core.runtime.block_on(async {
            let access_token = match core.token_store.get_access_token()? {
                Some(token) => token,
                None => return Ok::<(), anyhow::Error>(()),
            };

            let state = load_state(&core.state_path)?;
            let device_id = match state.device_id {
                Some(device_id) => device_id,
                None => return Ok::<(), anyhow::Error>(()),
            };

            let mut metadata = vec![("reason".to_string(), reason)];
            if let Some(detail) = detail {
                metadata.push(("detail".to_string(), detail));
            }

            api_client
                .create_alert_log(
                    &access_token,
                    &device_id,
                    &event_type,
                    &metadata,
                    Utc::now(),
                )
                .await?;

            Ok::<(), anyhow::Error>(())
        })
    })();

    to_jstring_result(&mut env, result)
}

fn core() -> Result<&'static AndroidCore> {
    CORE.get()
        .ok_or_else(|| anyhow!("native core not initialized"))
}

fn build_dynamic(token_store: Arc<dyn TokenStore>) -> Result<DynamicCore> {
    Ok(DynamicCore {
        auth_client: AuthClient::new(token_store.clone())?,
        api_client: ApiClient::new()?,
        upload_client: UploadClient::new()?,
        schedule_policy: CaptureSchedulePolicy {
            base_interval: Duration::from_secs(resolve_capture_interval_seconds()),
            ..CaptureSchedulePolicy::default()
        },
    })
}

fn refresh_dynamic(core: &AndroidCore) -> Result<()> {
    {
        let mut dynamic = core
            .dynamic
            .lock()
            .map_err(|_| anyhow!("dynamic core lock poisoned"))?;
        *dynamic = build_dynamic(core.token_store.clone())?;
    }

    let mut schedule_state = core
        .schedule_state
        .lock()
        .map_err(|_| anyhow!("schedule state lock poisoned"))?;
    *schedule_state = CaptureScheduleState::default();
    Ok(())
}

fn apply_overrides(base_api_url: &str, capture_interval_seconds: &str, batch_window_seconds: &str) {
    set_or_remove_env(BASE_API_URL_ENV_VAR, normalize_base_url(base_api_url));
    set_or_remove_env(
        CAPTURE_INTERVAL_SECONDS_ENV_VAR,
        normalize_numeric(capture_interval_seconds),
    );
    set_or_remove_env(
        BATCH_WINDOW_SECONDS_ENV_VAR,
        normalize_numeric(batch_window_seconds),
    );
}

fn normalize_base_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.trim_end_matches('/').to_string())
}

fn normalize_numeric(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

fn set_or_remove_env(key: &str, value: Option<String>) {
    match value {
        Some(v) => {
            unsafe { std::env::set_var(key, v) };
        }
        None => {
            unsafe { std::env::remove_var(key) };
        }
    }
}

async fn process_upload_queue(
    core: &AndroidCore,
    upload_client: &UploadClient,
    access_token: &str,
    device_id: &str,
    max_items: usize,
) -> Result<()> {
    let e2ee_key = core
        .token_store
        .get_e2ee_key()?
        .ok_or_else(|| anyhow!("E2EE key missing; sign in again"))?;

    let mut uploaded = 0usize;
    let mut rng = thread_rng();

    while uploaded < max_items {
        let now = Utc::now();
        if !core.queue.front_is_ready(now)? {
            break;
        }

        let Some(front) = core.queue.peek_front()? else {
            break;
        };
        let batch_window = TimeDelta::seconds(
            resolve_batch_window_seconds()
                .min(i64::MAX as u64)
                .try_into()
                .unwrap_or(i64::MAX),
        );
        if (now - front.taken_at) < batch_window {
            break;
        }

        let batch_item = BatchItem {
            ts: front.taken_at.timestamp_millis(),
            type_: "image".to_string(),
            data: BTreeMap::from([(
                "image".to_string(),
                BatchValue::Binary(front.payload.clone()),
            )]),
        };

        let blob = BatchBlob::new(vec![batch_item]);

        match upload_client
            .upload_batch(
                access_token,
                device_id,
                &blob,
                front.taken_at,
                front.taken_at,
                &e2ee_key,
            )
            .await
        {
            Ok(_) => {
                let _ = core.queue.pop_front()?;
                uploaded = uploaded.saturating_add(1);
            }
            Err(err) => {
                let next_delay = core
                    .retry_policy
                    .next_delay(front.attempts.saturating_add(1), &mut rng);
                let next_attempt_at = now
                    + TimeDelta::from_std(next_delay).unwrap_or_else(|_| TimeDelta::seconds(30));

                let updated = core.queue.mark_front_retry(next_attempt_at)?;
                if let Some(updated) = updated {
                    if updated.attempts >= core.retry_policy.max_attempts {
                        let _ = core.queue.pop_front()?;
                    }
                }

                return Err(anyhow!("upload failed: {err:#}"));
            }
        }
    }

    Ok(())
}

fn load_state(path: &Path) -> Result<AndroidState> {
    if !path.exists() {
        return Ok(AndroidState::default());
    }

    let raw = fs::read(path).with_context(|| format!("failed reading {}", path.display()))?;
    if raw.is_empty() {
        return Ok(AndroidState::default());
    }

    serde_json::from_slice(&raw).with_context(|| format!("failed parsing {}", path.display()))
}

fn save_state(path: &Path, state: &AndroidState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed creating {}", parent.display()))?;
    }

    let tmp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(state)?;
    fs::write(&tmp, bytes).with_context(|| format!("failed writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("failed replacing {}", path.display()))?;

    Ok(())
}

fn to_jstring_result(env: &mut JNIEnv, result: Result<()>) -> jstring {
    match result {
        Ok(_) => std::ptr::null_mut(),
        Err(err) => to_jstring(env, &err.to_string()),
    }
}

fn to_jstring(env: &mut JNIEnv, value: &str) -> jstring {
    match env.new_string(value) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}
