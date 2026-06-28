use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use jni::objects::{GlobalRef, JByteArray, JClass, JString, JValue};
use jni::sys::{jboolean, jstring};
use jni::{JNIEnv, JavaVM};
use once_cell::sync::OnceCell;
use serde::de::DeserializeOwned;
use virtue_core::{
    build_default_modules_reqwest, load_state, store_state, AuthState, Config, CoreError,
    CoreResult, DeviceSettings, EventBus, EventChannel, LoginRequested, LoginResult,
    LogoutRequested, Ping, PlatformHooks, ProcessStarted, ProcessStopped, ProcessStoppedReason,
    Redacted, Screenshot, ScreenshotHooks, StatusRequest, StatusResponse, UserStopRequested,
};

static CORE: OnceCell<AndroidCore> = OnceCell::new();

const DEFAULT_BASE_API_URL: &str = virtue_core::DEFAULT_API_BASE_URL;
const DEFAULT_CAPTURE_INTERVAL_SECONDS: u64 = 300;
const DEFAULT_BATCH_WINDOW_SECONDS: u64 = 3600;
const ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(20);
const LOOP_INTERVAL: Duration = Duration::from_secs(1);

const SCREENSHOT_SERVICE_CLASS: &str = "org/virtueinitiative/virtue/ScreenshotService";
const CAPTURE_STATUS_READY: i32 = 0;
const CAPTURE_STATUS_PERMISSION_MISSING: i32 = 1;
const CAPTURE_STATUS_SESSION_UNAVAILABLE: i32 = 2;

struct AndroidCore {
    state_dir: PathBuf,
    runtime_config_file: PathBuf,
    java_vm: Arc<JavaVM>,
    // Cached at init time (main thread) so background threads can use the app class loader.
    screenshot_service_class: Arc<GlobalRef>,
    stop: Arc<AtomicBool>,
    user_stop: Arc<AtomicBool>,
    daemon_running: Mutex<bool>,
}

#[derive(Clone)]
struct AndroidPlatformHooks {
    java_vm: Arc<JavaVM>,
    screenshot_service_class: Arc<GlobalRef>,
}

impl AndroidPlatformHooks {
    fn capture_status(&self) -> Result<i32, CoreError> {
        let mut env = self.java_vm.attach_current_thread().map_err(|err| {
            CoreError::CommandFailed(format!("attach_current_thread failed: {err}"))
        })?;

        env.call_static_method(
            &*self.screenshot_service_class,
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
            .call_static_method(
                &*self.screenshot_service_class,
                "capturePngForDaemon",
                "()[B",
                &[],
            )
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

impl ScreenshotHooks for AndroidPlatformHooks {
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

    fn get_last_shutdown_time_utc_ms(&self) -> CoreResult<Option<i64>> {
        Ok(None)
    }

    fn is_locked_or_screensaver(&self) -> CoreResult<bool> {
        let mut env = self.java_vm.attach_current_thread().map_err(|err| {
            CoreError::CommandFailed(format!("attach_current_thread failed: {err}"))
        })?;
        // Screen off (non-interactive) is the mobile equivalent of a locked/asleep
        // desktop. Fail-safe to `false` (treat as viewable → fall back to the diff gate)
        // when the state can't be read, never silently suppress.
        match is_interactive(&mut env) {
            Ok(interactive) => Ok(!interactive),
            Err(_) => Ok(false),
        }
    }

    fn get_last_startup_time_utc_ms(&self) -> CoreResult<Option<i64>> {
        let mut env = self.java_vm.attach_current_thread().map_err(|err| {
            CoreError::CommandFailed(format!("attach_current_thread failed: {err}"))
        })?;

        let uptime_ms: i64 = env
            .call_static_method("android/os/SystemClock", "elapsedRealtime", "()J", &[])
            .map_err(|err| {
                CoreError::CommandFailed(format!(
                    "failed to get boot time from system clock: {err}"
                ))
            })?
            .j()
            .map_err(|err| {
                CoreError::CommandFailed(format!(
                    "failed to get boot time from system clock: {err}"
                ))
            })?;

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|err| CoreError::CommandFailed(format!("system time error: {err}")))?
            .as_millis() as i64;

        Ok(Some(now_ms - uptime_ms))
    }
}

impl PlatformHooks for AndroidPlatformHooks {}

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
        sanitize_state_dir(Path::new(&data_dir))?;

        let runtime_config_file = Path::new(&config_dir).join("config.json");
        write_runtime_overrides(
            &runtime_config_file,
            &base_api_url,
            &capture_interval_seconds,
            &batch_window_seconds,
        )?;

        if CORE.get().is_none() {
            let java_vm = Arc::new(env.get_java_vm().context("failed to get JavaVM")?);
            // Cache the ScreenshotService class here (main thread → app class loader).
            // Background threads use the system class loader and cannot resolve app classes.
            let class = env
                .find_class(SCREENSHOT_SERVICE_CLASS)
                .context("failed to find ScreenshotService class")?;
            let screenshot_service_class = Arc::new(
                env.new_global_ref(class)
                    .context("failed to create GlobalRef for ScreenshotService")?,
            );
            CORE.set(AndroidCore {
                state_dir: PathBuf::from(data_dir),
                runtime_config_file,
                java_vm,
                screenshot_service_class,
                stop: Arc::new(AtomicBool::new(false)),
                user_stop: Arc::new(AtomicBool::new(false)),
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
        let (mut bus, state_path) = build_bus(core)?;
        let result = bus.request::<LoginRequested, LoginResult>(LoginRequested {
            email,
            password: Redacted(password),
            device_name: Some(device_name),
        })?;
        if !result.success {
            return Err(anyhow!(result.error.unwrap_or_else(|| {
                "Login failed. Check your credentials and try again.".to_string()
            })));
        }
        let state = bus.iter()?;
        store_state(&state_path, &state)?;
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
        let (mut bus, state_path) = build_bus(core)?;
        bus.send(LogoutRequested)?;
        let state = bus.iter()?;
        store_state(&state_path, &state)?;
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
        .ok()
        .and_then(|core| read_auth_state(&core.state_dir))
        .map(|auth| auth.device_credentials.is_some())
    {
        Some(true) => 1,
        _ => 0,
    }
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeGetDeviceId(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let device_id = core()
        .ok()
        .and_then(|core| read_auth_state(&core.state_dir))
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
        core.user_stop.store(false, Ordering::SeqCst);

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

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeNoteUserStop(
    mut env: JNIEnv,
    _class: JClass,
    source: JString,
) -> jstring {
    let result = (|| -> Result<()> {
        let core = core()?;
        let source: String = env.get_string(&source)?.into();
        let (mut bus, state_path) = build_bus(core)?;
        bus.send(UserStopRequested { source })?;
        let state = bus.iter()?;
        store_state(&state_path, &state)?;
        core.user_stop.store(true, Ordering::SeqCst);
        Ok(())
    })();

    to_jstring_result(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeGetStatusJson(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let json = (|| -> Result<String> {
        let core = core()?;
        let (mut bus, _) = build_bus(core)?;
        let response = bus.request::<StatusRequest, StatusResponse>(StatusRequest)?;
        Ok(serde_json::to_string(&response.status)?)
    })()
    .unwrap_or_else(|_| "{}".to_string());

    env.new_string(json)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

pub fn is_interactive(env: &mut JNIEnv) -> jni::errors::Result<bool> {
    let activity_thread = env
        .call_static_method(
            "android/app/ActivityThread",
            "currentApplication",
            "()Landroid/app/Application;",
            &[],
        )?
        .l()?;

    let power_service = env
        .get_static_field(
            "android/content/Context",
            "POWER_SERVICE",
            "Ljava/lang/String;",
        )?
        .l()?;

    let power_manager = env
        .call_method(
            &activity_thread,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[JValue::Object(&power_service)],
        )?
        .l()?;

    let interactive = env
        .call_method(&power_manager, "isInteractive", "()Z", &[])?
        .z()?;

    Ok(interactive)
}

fn build_bus(core: &AndroidCore) -> Result<(EventBus, PathBuf)> {
    let cfg = build_core_config(core);
    let modules = build_default_modules_reqwest(
        cfg,
        AndroidPlatformHooks {
            java_vm: core.java_vm.clone(),
            screenshot_service_class: core.screenshot_service_class.clone(),
        },
    )?;
    let state_path = core.state_dir.join("event_state.json");
    let bus = EventBus::new(modules, load_state(&state_path)?)?;
    Ok((bus, state_path))
}

fn run_daemon_loop(core: &AndroidCore) -> Result<()> {
    let (mut bus, state_path) = build_bus(core)?;
    bus.send(ProcessStarted)?;
    let state = bus.iter()?;
    store_state(&state_path, &state)?;

    while !core.stop.load(Ordering::SeqCst) {
        // Screen-off is now handled inside the bus via the `is_locked_or_screensaver`
        // hook: the screenshot module records a `ScreenshotSkipped` and the upload
        // module defers network I/O while the screen is off.
        let sleep_duration = match (|| -> Result<()> {
            bus.send(Ping)?;
            let state = bus.iter()?;
            store_state(&state_path, &state)?;
            Ok(())
        })() {
            Ok(()) => LOOP_INTERVAL,
            Err(err) => {
                eprintln!("android-daemon: {err}");
                ERROR_RETRY_INTERVAL
            }
        };
        sleep_interruptible(&core.stop, sleep_duration);
    }

    let reason = if core.user_stop.load(Ordering::SeqCst) {
        ProcessStoppedReason::User
    } else {
        ProcessStoppedReason::Other
    };
    bus.send(ProcessStopped(reason))?;
    let state = bus.iter()?;
    let _ = store_state(&state_path, &state);
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

fn build_core_config(core: &AndroidCore) -> Config {
    // The device name passed at construction is only a placeholder: device
    // registration happens on login, which carries the user-chosen name on the
    // `LoginRequested` event.
    Config::new(
        DEFAULT_BASE_API_URL,
        "android",
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

fn sanitize_state_dir(root: &Path) -> Result<()> {
    sanitize_json_file::<Option<DeviceSettings>>(root, "device_settings.json")?;
    Ok(())
}

fn sanitize_json_file<T: DeserializeOwned>(root: &Path, name: &str) -> Result<()> {
    let path = root.join(name);
    if !path.exists() {
        return Ok(());
    }

    let bytes = fs::read(&path).with_context(|| format!("failed reading {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(());
    }

    if serde_json::from_slice::<T>(&bytes).is_ok() {
        return Ok(());
    }

    fs::remove_file(&path).with_context(|| format!("failed removing {}", path.display()))?;
    Ok(())
}

fn read_auth_state(state_dir: &Path) -> Option<AuthState> {
    let path = state_dir.join("event_state.json");
    let bytes = fs::read(&path).ok()?;
    let state: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    serde_json::from_value(state.get("auth")?.clone()).ok()
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
