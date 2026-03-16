use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jboolean, jstring};
use jni::{JNIEnv, JavaVM};
use once_cell::sync::OnceCell;
use virtue_core::storage::FileStateStore;
use virtue_core::{Config, CoreError, CoreResult, MonitorService, PlatformHooks, Screenshot};

static CORE: OnceCell<AndroidCore> = OnceCell::new();

const DEFAULT_BASE_API_URL: &str = "https://api.virtueinitiative.org";
const DEFAULT_CAPTURE_INTERVAL_SECONDS: u64 = 300;
const DEFAULT_BATCH_WINDOW_SECONDS: u64 = 3600;
const ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(20);

const SCREENSHOT_SERVICE_CLASS: &str = "org/virtueinitiative/virtue/ScreenshotService";
const CAPTURE_STATUS_READY: i32 = 0;
const CAPTURE_STATUS_PERMISSION_MISSING: i32 = 1;
const CAPTURE_STATUS_SESSION_UNAVAILABLE: i32 = 2;

struct AndroidCore {
    state_dir: PathBuf,
    runtime_config_file: PathBuf,
    java_vm: Arc<JavaVM>,
    stop: Arc<AtomicBool>,
    daemon_running: Mutex<bool>,
}

#[derive(Clone)]
struct AndroidPlatformHooks {
    java_vm: Arc<JavaVM>,
}

impl AndroidPlatformHooks {
    fn capture_status(&self) -> Result<i32, CoreError> {
        let mut env = self.java_vm.attach_current_thread().map_err(|err| {
            CoreError::CommandFailed(format!("attach_current_thread failed: {err}"))
        })?;

        env.call_static_method(
            SCREENSHOT_SERVICE_CLASS,
            "captureStatusForDaemon",
            "()I",
            &[],
        )
        .map_err(|err| CoreError::CommandFailed(format!("captureStatusForDaemon failed: {err}")))?
        .i()
        .map_err(|err| {
            CoreError::CommandFailed(format!("captureStatusForDaemon type error: {err}"))
        })
    }

    fn capture_png(&self) -> Result<Vec<u8>, CoreError> {
        let mut env = self.java_vm.attach_current_thread().map_err(|err| {
            CoreError::CommandFailed(format!("attach_current_thread failed: {err}"))
        })?;

        let value = env
            .call_static_method(SCREENSHOT_SERVICE_CLASS, "capturePngForDaemon", "()[B", &[])
            .map_err(|err| {
                CoreError::CommandFailed(format!("capturePngForDaemon failed: {err}"))
            })?;
        let array_obj = value.l().map_err(|err| {
            CoreError::CommandFailed(format!("capturePngForDaemon type error: {err}"))
        })?;

        if array_obj.is_null() {
            return Err(CoreError::CommandFailed(
                "capture frame unavailable from ScreenshotService".to_string(),
            ));
        }

        let array = JByteArray::from(array_obj);
        env.convert_byte_array(&array)
            .map_err(|err| CoreError::CommandFailed(format!("decode capture byte[] failed: {err}")))
    }
}

impl PlatformHooks for AndroidPlatformHooks {
    fn take_screenshot(&self) -> CoreResult<Screenshot> {
        match self.capture_status()? {
            CAPTURE_STATUS_READY => {
                let bytes = self.capture_png()?;
                Ok(Screenshot {
                    captured_at_ms: self.get_time_utc_ms()?,
                    bytes,
                    content_type: "image/png".to_string(),
                })
            }
            CAPTURE_STATUS_PERMISSION_MISSING => Err(CoreError::CommandFailed(
                "capture permission missing".to_string(),
            )),
            CAPTURE_STATUS_SESSION_UNAVAILABLE => Err(CoreError::CommandFailed(
                "capture session unavailable".to_string(),
            )),
            other => Err(CoreError::CommandFailed(format!(
                "unexpected capture status code: {other}"
            ))),
        }
    }

    fn get_time_utc_ms(&self) -> CoreResult<i64> {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| CoreError::CommandFailed(err.to_string()))?;
        i64::try_from(duration.as_millis())
            .map_err(|_| CoreError::InvalidState("system clock overflow"))
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

        fs::create_dir_all(&config_dir)
            .with_context(|| format!("failed to create config dir {config_dir}"))?;
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("failed to create data dir {data_dir}"))?;

        let runtime_config_file = Path::new(&config_dir).join("config.json");
        write_runtime_overrides(
            &runtime_config_file,
            &base_api_url,
            &capture_interval_seconds,
            &batch_window_seconds,
        )?;

        if CORE.get().is_none() {
            let java_vm = Arc::new(env.get_java_vm().context("failed to get JavaVM")?);
            CORE.set(AndroidCore {
                state_dir: PathBuf::from(data_dir),
                runtime_config_file,
                java_vm,
                stop: Arc::new(AtomicBool::new(false)),
                daemon_running: Mutex::new(false),
            })
            .map_err(|_| anyhow!("core already initialized"))?;
        }

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
        let core = core()?;
        let base_api_url: String = env.get_string(&base_api_url)?.into();
        let capture_interval_seconds: String = env.get_string(&capture_interval_seconds)?.into();
        let batch_window_seconds: String = env.get_string(&batch_window_seconds)?.into();

        write_runtime_overrides(
            &core.runtime_config_file,
            &base_api_url,
            &capture_interval_seconds,
            &batch_window_seconds,
        )
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
        let email: String = env.get_string(&email)?.into();
        let password: String = env.get_string(&password)?.into();
        let device_name: String = env.get_string(&device_name)?.into();
        let core = core()?;
        let hooks = AndroidPlatformHooks {
            java_vm: core.java_vm.clone(),
        };
        let mut service = MonitorService::setup(build_core_config(core, &device_name), hooks)?;
        service.login(&email, &password)?;
        Ok(())
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
        let hooks = AndroidPlatformHooks {
            java_vm: core.java_vm.clone(),
        };
        let mut service = MonitorService::setup(build_core_config(core, "android-device"), hooks)?;
        service.logout()?;
        Ok(())
    })();

    to_jstring_result(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeIsLoggedIn(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    match core()
        .and_then(|core| Ok(FileStateStore::new(&core.state_dir)?.load_auth_state()?))
        .map(|auth| auth.device_credentials.is_some())
    {
        Ok(true) => 1,
        _ => 0,
    }
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeGetDeviceId(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let device_id = core()
        .and_then(|core| Ok(FileStateStore::new(&core.state_dir)?.load_auth_state()?))
        .ok()
        .and_then(|auth| auth.device_credentials.map(|device| device.device_id));

    match device_id {
        Some(value) => env
            .new_string(value)
            .map(|value| value.into_raw())
            .unwrap_or(std::ptr::null_mut()),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeRunDaemonLoop(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let result = (|| -> Result<()> {
        let core = core()?;
        {
            let mut guard = core
                .daemon_running
                .lock()
                .map_err(|_| anyhow!("daemon state lock poisoned"))?;
            if *guard {
                return Err(anyhow!("daemon already running"));
            }
            *guard = true;
        }
        core.stop.store(false, Ordering::SeqCst);

        let daemon_result = run_daemon_loop(core);

        if let Ok(mut guard) = core.daemon_running.lock() {
            *guard = false;
        }
        daemon_result
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
        core.stop.store(true, Ordering::SeqCst);
        Ok(())
    })();

    to_jstring_result(&mut env, result)
}

fn run_daemon_loop(core: &AndroidCore) -> Result<()> {
    let hooks = AndroidPlatformHooks {
        java_vm: core.java_vm.clone(),
    };
    let mut service = MonitorService::setup(build_core_config(core, "android-device"), hooks)?;

    while !core.stop.load(Ordering::SeqCst) {
        let sleep_duration = match service.loop_iteration() {
            Ok(outcome) => duration_until(outcome.next_run_at_ms),
            Err(err) => {
                eprintln!("android-daemon: {err}");
                ERROR_RETRY_INTERVAL
            }
        };
        sleep_interruptible(&core.stop, sleep_duration);
    }

    let _ = service.shutdown();
    Ok(())
}

fn sleep_interruptible(stop: &AtomicBool, duration: Duration) {
    let mut remaining = duration;
    while remaining > Duration::ZERO && !stop.load(Ordering::SeqCst) {
        let tick = remaining.min(Duration::from_secs(1));
        thread::sleep(tick);
        remaining = remaining.saturating_sub(tick);
    }
}

fn duration_until(next_run_at_ms: i64) -> Duration {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(next_run_at_ms);
    let delta_ms = next_run_at_ms.saturating_sub(now_ms);
    Duration::from_millis(delta_ms.max(0) as u64)
}

fn build_core_config(core: &AndroidCore, device_name: &str) -> Config {
    Config::new(
        DEFAULT_BASE_API_URL,
        device_name,
        "android",
        core.state_dir.clone(),
        Some(core.runtime_config_file.clone()),
        Duration::from_secs(DEFAULT_CAPTURE_INTERVAL_SECONDS),
        Duration::from_secs(DEFAULT_BATCH_WINDOW_SECONDS),
    )
}

fn write_runtime_overrides(
    path: &Path,
    base_api_url: &str,
    capture_interval_seconds: &str,
    batch_window_seconds: &str,
) -> Result<()> {
    let mut payload = serde_json::Map::new();
    if !base_api_url.trim().is_empty() {
        payload.insert(
            "api_base_url".to_string(),
            serde_json::Value::String(base_api_url.trim().to_string()),
        );
    }
    if !capture_interval_seconds.trim().is_empty() {
        payload.insert(
            "capture_interval_seconds".to_string(),
            serde_json::Value::Number(parse_u64(capture_interval_seconds)?.into()),
        );
    }
    if !batch_window_seconds.trim().is_empty() {
        payload.insert(
            "batch_window_seconds".to_string(),
            serde_json::Value::Number(parse_u64(batch_window_seconds)?.into()),
        );
    }

    let bytes = serde_json::to_vec_pretty(&serde_json::Value::Object(payload))?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).with_context(|| format!("failed writing {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed replacing {} with {}", path.display(), tmp.display()))?;
    Ok(())
}

fn parse_u64(value: &str) -> Result<u64> {
    value
        .trim()
        .parse::<u64>()
        .with_context(|| format!("invalid integer override: {value}"))
}

fn core() -> Result<&'static AndroidCore> {
    CORE.get().ok_or_else(|| anyhow!("core not initialized"))
}

fn to_jstring_result(env: &mut JNIEnv, result: Result<()>) -> jstring {
    match result {
        Ok(()) => std::ptr::null_mut(),
        Err(err) => env
            .new_string(err.to_string())
            .map(|value| value.into_raw())
            .unwrap_or(std::ptr::null_mut()),
    }
}
