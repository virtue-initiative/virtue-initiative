use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jboolean, jlong, jstring};
use once_cell::sync::OnceCell;
use rand::thread_rng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::runtime::Runtime;
use uuid::Uuid;

use virtue_client_core::{
    AuthClient, BufferedUpload, CaptureSchedulePolicy, CaptureScheduleState, FileTokenStore,
    ImagePipeline, PersistentQueue, RetryPolicy, TokenStore, UploadClient, resolve_base_api_url,
    resolve_capture_interval_seconds,
};

static CORE: OnceCell<AndroidCore> = OnceCell::new();

struct AndroidCore {
    runtime: Runtime,
    auth_client: AuthClient,
    token_store: Arc<dyn TokenStore>,
    upload_client: UploadClient,
    queue: PersistentQueue,
    pipeline: ImagePipeline,
    retry_policy: RetryPolicy,
    schedule_policy: CaptureSchedulePolicy,
    schedule_state: Mutex<CaptureScheduleState>,
    state_path: PathBuf,
    http_client: Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AndroidState {
    device_id: Option<String>,
}

impl Default for AndroidState {
    fn default() -> Self {
        Self { device_id: None }
    }
}

#[derive(Debug, Serialize)]
struct RegisterDeviceRequest {
    name: String,
    platform: String,
}

#[derive(Debug, Deserialize)]
struct RegisterDeviceResponse {
    id: String,
}

#[derive(Debug, Serialize)]
struct CreateLogRequest {
    #[serde(rename = "type")]
    event_type: String,
    device_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_id: Option<String>,
    metadata: BTreeMap<String, serde_json::Value>,
}

#[no_mangle]
pub extern "system" fn Java_codes_anb_virtue_NativeBridge_nativeInit(
    mut env: JNIEnv,
    _class: JClass,
    config_dir: JString,
    data_dir: JString,
) -> jstring {
    let result = (|| -> Result<()> {
        if CORE.get().is_some() {
            return Ok(());
        }

        let config_dir: String = env.get_string(&config_dir)?.into();
        let data_dir: String = env.get_string(&data_dir)?.into();

        fs::create_dir_all(&config_dir)
            .with_context(|| format!("failed to create config dir {config_dir}"))?;
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("failed to create data dir {data_dir}"))?;

        let token_file = Path::new(&config_dir).join("token_store.json");
        let queue_file = Path::new(&data_dir).join("upload_queue.json");
        let state_file = Path::new(&config_dir).join("android_client_state.json");

        let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&token_file));
        let auth_client = AuthClient::new(token_store.clone())?;
        let upload_client = UploadClient::new()?;
        let queue = PersistentQueue::open(&queue_file, 512)?;

        let runtime = Runtime::new().context("failed to build tokio runtime")?;
        let http_client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to build http client")?;

        CORE.set(AndroidCore {
            runtime,
            auth_client,
            token_store,
            upload_client,
            queue,
            pipeline: ImagePipeline::default(),
            retry_policy: RetryPolicy::default(),
            schedule_policy: CaptureSchedulePolicy {
                base_interval: Duration::from_secs(resolve_capture_interval_seconds()),
                ..CaptureSchedulePolicy::default()
            },
            schedule_state: Mutex::new(CaptureScheduleState::default()),
            state_path: state_file,
            http_client,
        })
        .map_err(|_| anyhow!("core already initialized"))?;

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

        core.runtime.block_on(async {
            core.auth_client.login(&email, &password).await?;
            let access_token = core
                .token_store
                .get_access_token()?
                .ok_or_else(|| anyhow!("missing access token after login"))?;

            let device_id = register_device(&core.http_client, &access_token, &device_name).await?;

            let mut state = load_state(&core.state_path)?;
            state.device_id = Some(device_id);
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

        core.runtime.block_on(async {
            let token = core.token_store.get_access_token()?;
            let state = load_state(&core.state_path)?;

            if let (Some(access_token), Some(device_id)) =
                (token.as_deref(), state.device_id.as_deref())
            {
                let mut metadata = BTreeMap::new();
                metadata.insert("reason".to_string(), json!("user_logout"));
                let _ = send_log(
                    &core.http_client,
                    access_token,
                    "manual_override",
                    device_id,
                    None,
                    metadata,
                )
                .await;
            }

            let _ = core.auth_client.logout().await;
            core.token_store.clear_access_token()?;
            core.token_store.clear_refresh_token()?;

            let mut new_state = state;
            new_state.device_id = None;
            save_state(&core.state_path, &new_state)?;

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
        let mut state = core
            .schedule_state
            .lock()
            .map_err(|_| anyhow!("schedule state lock poisoned"))?;
        let mut rng = thread_rng();

        let delay = core
            .schedule_policy
            .next_delay(&mut state, last_success != 0, &mut rng);

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
                device_id,
                Utc::now(),
                processed.content_type,
                processed.bytes,
                processed.sha256_hex,
            );
            core.queue.enqueue(item)?;

            core.upload_client
                .process_upload_queue(&core.queue, &core.retry_policy, &access_token, 12)
                .await?;

            Ok::<(), anyhow::Error>(())
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

            let mut metadata = BTreeMap::new();
            metadata.insert("reason".to_string(), json!(reason));
            if let Some(detail) = detail {
                metadata.insert("detail".to_string(), json!(detail));
            }

            send_log(
                &core.http_client,
                &access_token,
                &event_type,
                &device_id,
                None,
                metadata,
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

fn load_state(path: &Path) -> Result<AndroidState> {
    if !path.exists() {
        return Ok(AndroidState::default());
    }

    let raw = fs::read(path).with_context(|| format!("failed reading {}", path.display()))?;
    if raw.is_empty() {
        return Ok(AndroidState::default());
    }

    Ok(serde_json::from_slice(&raw)
        .with_context(|| format!("failed parsing {}", path.display()))?)
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

async fn register_device(client: &Client, access_token: &str, name: &str) -> Result<String> {
    let payload = RegisterDeviceRequest {
        name: name.to_string(),
        platform: "android".to_string(),
    };

    let base_url = resolve_base_api_url();
    let url = format!("{}/device", base_url);
    let response = client
        .post(url)
        .bearer_auth(access_token)
        .json(&payload)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("register_device failed ({status}): {body}"));
    }

    let body: RegisterDeviceResponse = response.json().await?;
    Ok(body.id)
}

async fn send_log(
    client: &Client,
    access_token: &str,
    event_type: &str,
    device_id: &str,
    image_id: Option<String>,
    metadata: BTreeMap<String, serde_json::Value>,
) -> Result<()> {
    let payload = CreateLogRequest {
        event_type: event_type.to_string(),
        device_id: device_id.to_string(),
        image_id,
        metadata,
    };

    let base_url = resolve_base_api_url();
    let url = format!("{}/log", base_url);
    let response = client
        .post(url)
        .bearer_auth(access_token)
        .json(&payload)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("send_log failed ({status}): {body}"));
    }

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
