use std::ffi::{CStr, CString};
use std::fs;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use tokio::runtime::{Builder, Runtime};
use tokio::time::sleep;

use virtue_client_core::{
    apply_dev_env, login_and_register_device, logout_and_clear_tokens_with_alert, run_batch_daemon,
    ApiClient, AuthClient, BatchDaemonConfig, CaptureOutcome, CoreError, FileTokenStore,
    LoginCommandInput, PersistedServiceState, ServiceEvent, ServiceHost,
    SleepOutcome, TokenStore, BASE_API_URL_ENV_VAR, BATCH_WINDOW_SECONDS_ENV_VAR,
    CAPTURE_INTERVAL_SECONDS_ENV_VAR,
};

static CORE: OnceCell<IosCore> = OnceCell::new();

const CAPTURE_STATUS_READY: c_int = 0;
const CAPTURE_STATUS_PERMISSION_MISSING: c_int = 1;
const CAPTURE_STATUS_SESSION_UNAVAILABLE: c_int = 2;

unsafe extern "C" {
    fn virtue_ios_capture_status() -> c_int;
    fn virtue_ios_capture_png_write(out_ptr: *mut *const u8, out_len: *mut usize) -> c_int;
    fn virtue_ios_capture_png_release(ptr: *const u8, len: usize);
}

struct IosCore {
    runtime: Runtime,
    token_store: Arc<dyn TokenStore>,
    state_path: PathBuf,
    batch_buffer_path: PathBuf,
    dynamic: Mutex<DynamicCore>,
    daemon_state: Mutex<DaemonState>,
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
struct IosState {
    #[serde(default = "default_monitoring_enabled")]
    monitoring_enabled: bool,
    device_id: Option<String>,
}

fn default_monitoring_enabled() -> bool {
    true
}

impl Default for IosState {
    fn default() -> Self {
        Self {
            monitoring_enabled: default_monitoring_enabled(),
            device_id: None,
        }
    }
}

#[derive(Clone)]
struct IosDaemonHost {
    state_path: PathBuf,
    stop: Arc<AtomicBool>,
}

impl IosDaemonHost {
    fn capture_status(&self) -> c_int {
        // SAFETY: exported by the iOS app binary; no pointer parameters.
        unsafe { virtue_ios_capture_status() }
    }

    fn capture_png(&self) -> Result<Vec<u8>, CoreError> {
        let mut ptr: *const u8 = std::ptr::null();
        let mut len: usize = 0;

        // SAFETY: callback writes pointer/length pair into valid out parameters.
        let rc = unsafe { virtue_ios_capture_png_write(&mut ptr, &mut len) };
        if rc != 0 {
            return Err(CoreError::Platform(format!(
                "capture callback returned error code {rc}"
            )));
        }
        if ptr.is_null() || len == 0 {
            return Err(CoreError::Platform(
                "capture callback returned empty frame".to_string(),
            ));
        }

        // SAFETY: callback guarantees pointer is valid for len bytes until release.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len).to_vec() };
        // SAFETY: pointer came from virtue_ios_capture_png_write and must be released by callback.
        unsafe { virtue_ios_capture_png_release(ptr, len) };
        Ok(bytes)
    }
}

impl ServiceHost for IosDaemonHost {
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
        match self.capture_status() {
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
            ServiceEvent::Info(msg) => eprintln!("ios-daemon: {msg}"),
            ServiceEvent::Warn(msg) => eprintln!("ios-daemon: {msg}"),
            ServiceEvent::Error(msg) => eprintln!("ios-daemon: {msg}"),
        }
    }

    fn should_stop(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_init(
    config_dir: *const c_char,
    data_dir: *const c_char,
    base_api_url: *const c_char,
    capture_interval_seconds: *const c_char,
    batch_window_seconds: *const c_char,
) -> *mut c_char {
    let result = (|| -> Result<()> {
        let config_dir = c_string_or_empty(config_dir);
        let data_dir = c_string_or_empty(data_dir);
        let base_api_url = c_string_or_empty(base_api_url);
        let capture_interval_seconds = c_string_or_empty(capture_interval_seconds);
        let batch_window_seconds = c_string_or_empty(batch_window_seconds);

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
        let state_file = Path::new(&config_dir).join("ios_client_state.json");
        let batch_buffer_file = Path::new(&data_dir).join("batch_buffer.json");

        let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&token_file));
        let runtime = Runtime::new().context("failed to build tokio runtime")?;
        let dynamic = build_dynamic(token_store.clone())?;

        CORE.set(IosCore {
            runtime,
            token_store,
            state_path: state_file,
            batch_buffer_path: batch_buffer_file,
            dynamic: Mutex::new(dynamic),
            daemon_state: Mutex::new(DaemonState::default()),
        })
        .map_err(|_| anyhow!("core already initialized"))?;

        Ok(())
    })();

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_set_overrides(
    base_api_url: *const c_char,
    capture_interval_seconds: *const c_char,
    batch_window_seconds: *const c_char,
) -> *mut c_char {
    let result = (|| -> Result<()> {
        let base_api_url = c_string_or_empty(base_api_url);
        let capture_interval_seconds = c_string_or_empty(capture_interval_seconds);
        let batch_window_seconds = c_string_or_empty(batch_window_seconds);

        apply_overrides(
            &base_api_url,
            &capture_interval_seconds,
            &batch_window_seconds,
        );

        let core = core()?;
        refresh_dynamic(core)?;
        Ok(())
    })();

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_login(
    email: *const c_char,
    password: *const c_char,
    device_name: *const c_char,
) -> *mut c_char {
    let result = (|| -> Result<()> {
        let core = core()?;
        let email = c_string_or_empty(email);
        let password = c_string_or_empty(password);
        let device_name = c_string_or_empty(device_name);

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
                    platform: "ios",
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

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_logout() -> *mut c_char {
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
                ("source".to_string(), "ios".to_string()),
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

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_run_daemon_loop() -> *mut c_char {
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
            let host = IosDaemonHost {
                state_path: core.state_path.clone(),
                stop: stop_signal,
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

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_stop_daemon() -> *mut c_char {
    let result = (|| -> Result<()> {
        let core = core()?;
        let daemon_state = core
            .daemon_state
            .lock()
            .map_err(|_| anyhow!("daemon state lock poisoned"))?;
        daemon_state.stop.store(true, Ordering::SeqCst);
        Ok(())
    })();

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_is_logged_in() -> bool {
    let result = (|| -> Result<bool> {
        let core = core()?;
        let token = core.token_store.get_access_token()?;
        let state = load_state(&core.state_path)?;
        Ok(token.is_some() && state.device_id.is_some())
    })();

    matches!(result, Ok(true))
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_get_device_id() -> *mut c_char {
    let result = (|| -> Result<Option<String>> {
        let core = core()?;
        let state = load_state(&core.state_path)?;
        Ok(state.device_id)
    })();

    match result {
        Ok(Some(device_id)) => to_c_string_ptr(&device_id),
        _ => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn virtue_ios_free_string(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    // SAFETY: value must be a pointer previously returned by CString::into_raw in this crate.
    unsafe {
        let _ = CString::from_raw(value);
    }
}

fn core() -> Result<&'static IosCore> {
    CORE.get()
        .ok_or_else(|| anyhow!("native core not initialized"))
}

fn build_dynamic(token_store: Arc<dyn TokenStore>) -> Result<DynamicCore> {
    Ok(DynamicCore {
        auth_client: AuthClient::new(token_store.clone())?,
        api_client: ApiClient::new()?,
    })
}

fn refresh_dynamic(core: &IosCore) -> Result<()> {
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
        Some(v) => unsafe { std::env::set_var(key, v) },
        None => unsafe { std::env::remove_var(key) },
    }
}

fn load_state(path: &Path) -> Result<IosState> {
    if !path.exists() {
        return Ok(IosState::default());
    }

    let raw = fs::read(path).with_context(|| format!("failed reading {}", path.display()))?;
    if raw.is_empty() {
        return Ok(IosState::default());
    }

    serde_json::from_slice(&raw).with_context(|| format!("failed parsing {}", path.display()))
}

fn save_state(path: &Path, state: &IosState) -> Result<()> {
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

fn c_string_or_empty(value: *const c_char) -> String {
    if value.is_null() {
        return String::new();
    }
    // SAFETY: pointer must reference a valid NUL-terminated string for the duration of this call.
    unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned()
}

fn into_c_result(result: Result<()>) -> *mut c_char {
    match result {
        Ok(_) => std::ptr::null_mut(),
        Err(err) => to_c_string_ptr(&err.to_string()),
    }
}

fn to_c_string_ptr(value: &str) -> *mut c_char {
    let sanitized = value.replace('\0', " ");
    match CString::new(sanitized) {
        Ok(cstr) => cstr.into_raw(),
        Err(_) => CString::new("failed to encode string")
            .expect("CString::new on static string cannot fail")
            .into_raw(),
    }
}
