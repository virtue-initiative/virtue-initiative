use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jboolean, jstring};
use jni::{JNIEnv, JavaVM};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use tokio::runtime::{Builder, Runtime};
use tokio::time::sleep;

use virtue_client_core::{
    apply_dev_env, login_and_register_device, logout_and_clear_tokens_with_alert, run_batch_daemon,
    ApiClient, AuthClient, BatchDaemonConfig, CaptureOutcome, CoreError, FileTokenStore,
    LoginCommandInput, PersistedServiceState, ServiceEvent, ServiceHost, SleepOutcome, TokenStore,
    BASE_API_URL_ENV_VAR, BATCH_WINDOW_SECONDS_ENV_VAR, CAPTURE_INTERVAL_SECONDS_ENV_VAR,
};

static CORE: OnceCell<AndroidCore> = OnceCell::new();

const SCREENSHOT_SERVICE_CLASS: &str = "org/virtueinitiative/virtue/ScreenshotService";
const CAPTURE_STATUS_READY: i32 = 0;
const CAPTURE_STATUS_PERMISSION_MISSING: i32 = 1;
const CAPTURE_STATUS_SESSION_UNAVAILABLE: i32 = 2;

struct AndroidCore {
    runtime: Runtime,
    token_store: Arc<dyn TokenStore>,
    state_path: PathBuf,
    batch_buffer_path: PathBuf,
    dynamic: Mutex<DynamicCore>,
    daemon_state: Mutex<DaemonState>,
    java_vm: Arc<JavaVM>,
}

#[derive(Clone)]
struct DynamicCore {
    auth_client: AuthClient,
    api_client: ApiClient,
}

struct DaemonState {
    running: bool,
    stop: Arc<AtomicBool>,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self {
            running: false,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AndroidState {
    #[serde(default = "default_monitoring_enabled")]
    monitoring_enabled: bool,
    device_id: Option<String>,
}

fn default_monitoring_enabled() -> bool {
    true
}

impl Default for AndroidState {
    fn default() -> Self {
        Self {
            monitoring_enabled: default_monitoring_enabled(),
            device_id: None,
        }
    }
}

#[derive(Clone)]
struct AndroidDaemonHost {
    state_path: PathBuf,
    stop: Arc<AtomicBool>,
    java_vm: Arc<JavaVM>,
}

impl AndroidDaemonHost {
    fn capture_status(&self) -> Result<i32, CoreError> {
        let mut env = self
            .java_vm
            .attach_current_thread()
            .map_err(|err| CoreError::Platform(format!("attach_current_thread failed: {err}")))?;

        env.call_static_method(
            SCREENSHOT_SERVICE_CLASS,
            "captureStatusForDaemon",
            "()I",
            &[],
        )
        .map_err(|err| CoreError::Platform(format!("captureStatusForDaemon failed: {err}")))?
        .i()
        .map_err(|err| CoreError::Platform(format!("captureStatusForDaemon type error: {err}")))
    }

    fn capture_png(&self) -> Result<Vec<u8>, CoreError> {
        let mut env = self
            .java_vm
            .attach_current_thread()
            .map_err(|err| CoreError::Platform(format!("attach_current_thread failed: {err}")))?;

        let value = env
            .call_static_method(SCREENSHOT_SERVICE_CLASS, "capturePngForDaemon", "()[B", &[])
            .map_err(|err| CoreError::Platform(format!("capturePngForDaemon failed: {err}")))?;
        let array_obj = value
            .l()
            .map_err(|err| CoreError::Platform(format!("capturePngForDaemon type error: {err}")))?;

        if array_obj.is_null() {
            return Err(CoreError::Platform(
                "capture frame unavailable from ScreenshotService".to_string(),
            ));
        }

        let array = JByteArray::from(array_obj);
        env.convert_byte_array(&array)
            .map_err(|err| CoreError::Platform(format!("decode capture byte[] failed: {err}")))
    }
}

impl ServiceHost for AndroidDaemonHost {
    fn load_persisted_state(&self) -> virtue_client_core::CoreResult<PersistedServiceState> {
        let state =
            load_state(&self.state_path).map_err(|err| CoreError::Platform(err.to_string()))?;
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
        match self.capture_status()? {
            CAPTURE_STATUS_READY => {
                let png = self.capture_png()?;
                Ok(CaptureOutcome::FramePng(png))
            }
            CAPTURE_STATUS_PERMISSION_MISSING => Ok(CaptureOutcome::PermissionMissing),
            CAPTURE_STATUS_SESSION_UNAVAILABLE => Ok(CaptureOutcome::SessionUnavailable),
            other => Err(CoreError::Platform(format!(
                "unexpected capture status code: {other}"
            ))),
        }
    }

    fn emit_event(&self, event: ServiceEvent) {
        match event {
            ServiceEvent::Info(msg) => eprintln!("android-daemon: {msg}"),
            ServiceEvent::Warn(msg) => eprintln!("android-daemon: {msg}"),
            ServiceEvent::Error(msg) => eprintln!("android-daemon: {msg}"),
        }
    }

    fn should_stop(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeInit(
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
        let state_file = Path::new(&config_dir).join("android_client_state.json");
        let batch_buffer_file = Path::new(&data_dir).join("batch_buffer.json");

        let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&token_file));
        let runtime = Runtime::new().context("failed to build tokio runtime")?;
        let dynamic = build_dynamic(token_store.clone())?;
        let java_vm = Arc::new(env.get_java_vm().context("failed to get JavaVM")?);

        CORE.set(AndroidCore {
            runtime,
            token_store,
            state_path: state_file,
            batch_buffer_path: batch_buffer_file,
            dynamic: Mutex::new(dynamic),
            daemon_state: Mutex::new(DaemonState::default()),
            java_vm,
        })
        .map_err(|_| anyhow!("core already initialized"))?;

        Ok(())
    })();

    to_jstring_result(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeSetOverrides(
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
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeLogin(
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
            state.monitoring_enabled = true;
            state.device_id = Some(login.device_id);
            save_state(&core.state_path, &state)?;
            Ok::<(), anyhow::Error>(())
        })
    })();

    to_jstring_result(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeLogout(
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

            state.monitoring_enabled = false;
            state.device_id = None;
            save_state(&core.state_path, &state)?;
            Ok::<(), anyhow::Error>(())
        })
    })();

    to_jstring_result(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeRunDaemonLoop(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let result = (|| -> Result<()> {
        let core = core()?;

        let stop_signal = {
            let mut daemon_state = core
                .daemon_state
                .lock()
                .map_err(|_| anyhow!("daemon state lock poisoned"))?;
            if daemon_state.running {
                return Ok(());
            }
            daemon_state.running = true;
            daemon_state.stop.store(false, Ordering::SeqCst);
            daemon_state.stop.clone()
        };

        let run_result = (|| -> Result<()> {
            let host = AndroidDaemonHost {
                state_path: core.state_path.clone(),
                stop: stop_signal,
                java_vm: core.java_vm.clone(),
            };
            let auth_client = AuthClient::new(core.token_store.clone())?;
            let api_client = ApiClient::new()?;
            let config = BatchDaemonConfig {
                settings_refresh_interval: Duration::from_secs(30 * 60),
                settings_fetch_retry_interval: Duration::from_secs(20),
                idle_retry_interval: Duration::from_secs(20),
                token_refresh_threshold: Duration::from_secs(120),
                session_unavailable_log_interval: Duration::from_secs(5 * 60),
                continue_on_token_refresh_error: false,
            };

            let daemon_runtime = Builder::new_current_thread()
                .enable_all()
                .build()
                .context("failed to build daemon runtime")?;

            daemon_runtime
                .block_on(run_batch_daemon(
                    &host,
                    core.token_store.clone(),
                    &auth_client,
                    &api_client,
                    &core.batch_buffer_path,
                    config,
                ))
                .map_err(Into::into)
        })();

        if let Ok(mut daemon_state) = core.daemon_state.lock() {
            daemon_state.running = false;
            daemon_state.stop.store(false, Ordering::SeqCst);
        }

        run_result
    })();

    to_jstring_result(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeStopDaemon(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let result = (|| -> Result<()> {
        let core = core()?;
        let daemon_state = core
            .daemon_state
            .lock()
            .map_err(|_| anyhow!("daemon state lock poisoned"))?;
        daemon_state.stop.store(true, Ordering::SeqCst);
        Ok(())
    })();

    to_jstring_result(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeIsLoggedIn(
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
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeGetDeviceId(
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
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeReportLog(
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
    })
}

fn refresh_dynamic(core: &AndroidCore) -> Result<()> {
    let mut dynamic = core
        .dynamic
        .lock()
        .map_err(|_| anyhow!("dynamic core lock poisoned"))?;
    *dynamic = build_dynamic(core.token_store.clone())?;
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
