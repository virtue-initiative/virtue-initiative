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
use virtue_text_detection::OcrError;
use once_cell::sync::OnceCell;
use serde::de::DeserializeOwned;
use virtue_core::{
    build_default_modules_reqwest, load_state, store_state, AuthState, Config, CoreError,
    CoreResult, DeviceSettings, EventBus, EventChannel, LifecycleHooks, LoginRequested,
    LoginResult, LogoutRequested, Ping, PlatformConfig, PlatformHooks, ProcessStarted,
    ProcessStopped, Redacted, Screenshot, ScreenshotHooks, StatusRequest, StatusResponse,
    UserStopRequested,
};

static CORE: OnceCell<AndroidCore> = OnceCell::new();
// Kept alive for the process lifetime; dropping it would silently stop the
// background thread that flushes buffered log lines. A dedicated `OnceCell`
// (rather than piggybacking on `CORE`, which has no room to hold a guard)
// still only ever runs its init closure once per process.
static LOG_GUARD: OnceCell<tracing_appender::non_blocking::WorkerGuard> = OnceCell::new();

const DEFAULT_BASE_API_URL: &str = virtue_core::DEFAULT_API_BASE_URL;
const ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(20);
const LOOP_INTERVAL: Duration = Duration::from_secs(1);

const SCREENSHOT_SERVICE_CLASS: &str = "org/virtueinitiative/virtue/ScreenshotService";
const CAPTURE_STATUS_READY: i32 = 0;
const CAPTURE_STATUS_PERMISSION_MISSING: i32 = 1;
const CAPTURE_STATUS_SESSION_UNAVAILABLE: i32 = 2;

struct AndroidCore {
    state_dir: PathBuf,
    java_vm: Arc<JavaVM>,
    // Cached at init time (main thread) so background threads can use the app class loader.
    screenshot_service_class: Arc<GlobalRef>,
    stop: Arc<AtomicBool>,
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
}

impl AndroidPlatformHooks {
    /// `SystemClock.elapsedRealtime()`: milliseconds since boot, INCLUDING time
    /// spent asleep/Doze.
    fn elapsed_realtime_ms(&self) -> CoreResult<i64> {
        let mut env = self.java_vm.attach_current_thread().map_err(|err| {
            CoreError::CommandFailed(format!("attach_current_thread failed: {err}"))
        })?;
        env.call_static_method("android/os/SystemClock", "elapsedRealtime", "()J", &[])
            .map_err(|err| CoreError::CommandFailed(format!("elapsedRealtime failed: {err}")))?
            .j()
            .map_err(|err| CoreError::CommandFailed(format!("elapsedRealtime type error: {err}")))
    }

    /// `SystemClock.uptimeMillis()`: milliseconds since boot, EXCLUDING deep-sleep
    /// time when the CPU was fully suspended.
    fn uptime_millis(&self) -> CoreResult<i64> {
        let mut env = self.java_vm.attach_current_thread().map_err(|err| {
            CoreError::CommandFailed(format!("attach_current_thread failed: {err}"))
        })?;
        env.call_static_method("android/os/SystemClock", "uptimeMillis", "()J", &[])
            .map_err(|err| CoreError::CommandFailed(format!("uptimeMillis failed: {err}")))?
            .j()
            .map_err(|err| CoreError::CommandFailed(format!("uptimeMillis type error: {err}")))
    }
}

impl LifecycleHooks for AndroidPlatformHooks {
    fn get_boot_clock_ms(&self) -> CoreResult<i64> {
        self.elapsed_realtime_ms()
    }

    fn get_monotonic_clock_ms(&self) -> CoreResult<i64> {
        self.uptime_millis()
    }

    // Android has no OS "login" concept; the expected-running window is modeled
    // as "whenever the device is powered on", so login = device boot time.
    fn get_last_login_utc_ms(&self) -> CoreResult<Option<i64>> {
        let uptime_ms = self.elapsed_realtime_ms()?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|err| CoreError::CommandFailed(format!("system time error: {err}")))?
            .as_millis() as i64;
        Ok(Some(now_ms - uptime_ms))
    }

    // Android gives a foreground service no reliable last-alive record — the
    // unexpected-stop bucket simply never fires here, an accepted gap rather
    // than a false negative being papered over.
    fn get_last_logout_utc_ms(&self) -> CoreResult<Option<i64>> {
        Ok(None)
    }
}

impl PlatformHooks for AndroidPlatformHooks {}

/// Installs the process-wide `tracing` subscriber on first call, writing
/// daily-rotated plain-text logs to `<data_dir>/logs/virtue.log`. Subsequent
/// calls are no-ops. No runtime override (no `RUST_LOG` on mobile) — the
/// compiled-in default filter for the build type is used directly.
fn init_logging(data_dir: &Path) {
    LOG_GUARD.get_or_init(|| {
        let log_dir = data_dir.join("logs");
        if let Err(err) = fs::create_dir_all(&log_dir) {
            eprintln!("failed to create logs dir {}: {err}", log_dir.display());
        }
        if let Err(err) = virtue_core::logging::prune_old_logs(
            &log_dir,
            &virtue_core::logging::DEFAULT_FILE_LOG_POLICY,
        ) {
            eprintln!("failed to prune old logs: {err}");
        }

        let file_appender = tracing_appender::rolling::daily(
            &log_dir,
            virtue_core::logging::DEFAULT_FILE_LOG_POLICY.file_name_prefix,
        );
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new(
                virtue_core::logging::default_filter_directive(cfg!(debug_assertions)),
            ))
            .with_writer(non_blocking)
            .with_ansi(false)
            .init();

        guard
    });
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeInit(
    mut env: JNIEnv,
    _class: JClass,
    config_dir: JString,
    data_dir: JString,
) -> jstring {
    let result = (|| -> Result<()> {
        let config_dir: String = env.get_string(&config_dir)?.into();
        let data_dir: String = env.get_string(&data_dir)?.into();

        fs::create_dir_all(&config_dir)
            .with_context(|| format!("failed to create config dir {config_dir}"))?;
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("failed to create data dir {data_dir}"))?;
        sanitize_state_dir(Path::new(&data_dir))?;

        if CORE.get().is_none() {
            init_logging(Path::new(&data_dir));

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

            // Register the OCR callback (once; OnceLock ignores repeat calls).
            // The class is cached here on the main thread; background threads use the
            // GlobalRef so the app class loader is not needed at call time.
            let virtue_ocr_class = env
                .find_class("org/virtueinitiative/virtue/VirtueOcr")
                .context("failed to find VirtueOcr class")?;
            let virtue_ocr_global = Arc::new(
                env.new_global_ref(virtue_ocr_class)
                    .context("failed to create GlobalRef for VirtueOcr")?,
            );
            let vm_for_ocr = java_vm.clone();
            virtue_text_detection::android::register_recognize_fn(move |image, language| {
                let mut ocr_env = vm_for_ocr
                    .attach_current_thread()
                    .map_err(|e| OcrError::Init(e.to_string()))?;
                let j_bytes = ocr_env
                    .byte_array_from_slice(image)
                    .map_err(|e| OcrError::Recognition(e.to_string()))?;
                let j_lang = ocr_env
                    .new_string(language.unwrap_or(""))
                    .map_err(|e| OcrError::Recognition(e.to_string()))?;
                let result = ocr_env
                    .call_static_method(
                        &*virtue_ocr_global,
                        "recognizeText",
                        "([BLjava/lang/String;)Ljava/lang/String;",
                        &[JValue::Object(&*j_bytes), JValue::Object(&*j_lang)],
                    )
                    .map_err(|e| OcrError::Recognition(e.to_string()))?;
                let j_str_obj = result
                    .l()
                    .map_err(|e| OcrError::Recognition(e.to_string()))?;
                let output = unsafe { ocr_env.get_string(&JString::from(j_str_obj)) }
                    .map_err(|e| OcrError::Recognition(e.to_string()))?
                    .to_string_lossy()
                    .into_owned();
                Ok(output)
            });

            CORE.set(AndroidCore {
                state_dir: PathBuf::from(data_dir),
                java_vm,
                screenshot_service_class,
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
    // Default `PlatformConfig` (lifecycle_enabled: true) is correct here:
    // unlike iOS, Android has a working boot/monotonic clock pair and a
    // reasonable login-window proxy (device boot time), so the full lifecycle
    // model applies.
    let modules = build_default_modules_reqwest(
        cfg,
        AndroidPlatformHooks {
            java_vm: core.java_vm.clone(),
            screenshot_service_class: core.screenshot_service_class.clone(),
        },
        PlatformConfig::default(),
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
        if let Err(err) = (|| -> Result<()> {
            bus.send(Ping)?;
            let state = bus.iter()?;
            store_state(&state_path, &state)?;
            Ok(())
        })() {
            tracing::error!(error = %err, "android-daemon");
        }
        sleep_interruptible(&core.stop, LOOP_INTERVAL);
    }

    bus.send(ProcessStopped)?;
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
        Duration::from_secs(virtue_core::default_capture_interval_seconds()),
        Duration::from_secs(virtue_core::default_batch_window_seconds()),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_logging_is_idempotent_across_repeated_calls() {
        // `nativeInit` can legitimately be called more than once per process
        // (e.g. the app re-initializing after a config change), and its
        // one-time-setup block guards on `CORE.get().is_none()` before calling
        // `init_logging`. `init_logging` itself must also tolerate more than
        // one call without panicking, since nothing prevents it being reached
        // twice before `CORE` is set on a slow init path.
        let dir = std::env::temp_dir().join(format!(
            "virtue-android-logging-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        init_logging(&dir);
        init_logging(&dir);

        assert!(LOG_GUARD.get().is_some(), "logging should be initialized");

        std::fs::remove_dir_all(&dir).ok();
    }
}
