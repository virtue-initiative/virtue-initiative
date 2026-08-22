use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{Global, JByteArray, JClass, JString, JValue};
use jni::sys::{jboolean, jstring};
use jni::{jni_sig, jni_str, Env, EnvUnowned, JavaVM};
use once_cell::sync::OnceCell;
use serde::de::DeserializeOwned;
use virtue_core::api::HttpApiClient;
use virtue_core::{
    AuthState, Config, CoreError, CoreResult, Daemon, DeviceSettings, LifecycleHooks, Screenshot,
    ScreenshotHooks,
};
use virtue_text_detection::OcrError;

static CORE: OnceCell<AndroidCore> = OnceCell::new();
// Kept alive for the process lifetime; dropping it would silently stop the
// background thread that flushes buffered log lines. A dedicated `OnceCell`
// (rather than piggybacking on `CORE`, which has no room to hold a guard)
// still only ever runs its init closure once per process.
static LOG_GUARD: OnceCell<tracing_appender::non_blocking::WorkerGuard> = OnceCell::new();

const DEFAULT_BASE_API_URL: &str = virtue_core::DEFAULT_API_BASE_URL;

const CAPTURE_STATUS_READY: i32 = 0;
const CAPTURE_STATUS_PERMISSION_MISSING: i32 = 1;
const CAPTURE_STATUS_SESSION_UNAVAILABLE: i32 = 2;

type AndroidDaemon = Daemon<AndroidPlatformHooks, HttpApiClient>;

struct AndroidCore {
    state_dir: PathBuf,
    daemon: Arc<AndroidDaemon>,
    daemon_running: Mutex<bool>,
}

#[derive(Clone)]
struct AndroidPlatformHooks {
    java_vm: Arc<JavaVM>,
    screenshot_service_class: Arc<Global<JClass<'static>>>,
}

/// jni 0.22's attachment APIs require the closure's error type to be
/// `From<jni::errors::Error>`, so JNI failures and our own domain failures
/// (e.g. the service handing back a null frame) share one type inside the
/// closure and are flattened into `CoreError` at the boundary.
#[derive(Debug)]
enum CallError {
    Jni(jni::errors::Error),
    Message(String),
}

impl From<jni::errors::Error> for CallError {
    fn from(err: jni::errors::Error) -> Self {
        Self::Jni(err)
    }
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Jni(err) => write!(f, "{err}"),
            Self::Message(msg) => write!(f, "{msg}"),
        }
    }
}

impl AndroidPlatformHooks {
    fn capture_status(&self) -> Result<i32, CoreError> {
        self.java_vm
            .attach_current_thread(|env| -> Result<i32, CallError> {
                Ok(env
                    .call_static_method(
                        &*self.screenshot_service_class,
                        jni_str!("captureStatusForDaemon"),
                        jni_sig!("()I"),
                        &[],
                    )?
                    .i()?)
            })
            .map_err(|err| {
                CoreError::CommandFailed(format!("captureStatusForDaemon failed: {err}"))
            })
    }

    fn capture_png(&self) -> Result<Vec<u8>, CoreError> {
        self.java_vm
            .attach_current_thread(|env| -> Result<Vec<u8>, CallError> {
                let array_obj = env
                    .call_static_method(
                        &*self.screenshot_service_class,
                        jni_str!("capturePngForDaemon"),
                        jni_sig!("()[B"),
                        &[],
                    )?
                    .l()?;

                if array_obj.is_null() {
                    return Err(CallError::Message(
                        "capture frame unavailable from ScreenshotService".to_string(),
                    ));
                }

                let array = env.cast_local::<JByteArray>(array_obj)?;
                Ok(env.convert_byte_array(&array)?)
            })
            .map_err(|err| CoreError::CommandFailed(format!("capturePngForDaemon failed: {err}")))
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
        // Screen off (non-interactive) is the mobile equivalent of a locked/asleep
        // desktop. Fail-safe to `false` (treat as viewable → fall back to the diff gate)
        // when the state can't be read, never silently suppress.
        let interactive =
            self.java_vm
                .attach_current_thread(|env| -> Result<bool, jni::errors::Error> {
                    is_interactive(env)
                });
        match interactive {
            Ok(interactive) => Ok(!interactive),
            Err(_) => Ok(false),
        }
    }
}

impl AndroidPlatformHooks {
    /// `SystemClock.elapsedRealtime()`: milliseconds since boot, INCLUDING time
    /// spent asleep/Doze. Used below as the login-window proxy (Android has no
    /// OS "login" concept — see `get_last_login_utc_ms`).
    fn elapsed_realtime_ms(&self) -> CoreResult<i64> {
        self.java_vm
            .attach_current_thread(|env| -> Result<i64, jni::errors::Error> {
                env.call_static_method(
                    jni_str!("android/os/SystemClock"),
                    jni_str!("elapsedRealtime"),
                    jni_sig!("()J"),
                    &[],
                )?
                .j()
            })
            .map_err(|err| CoreError::CommandFailed(format!("elapsedRealtime failed: {err}")))
    }

    /// `SystemClock.uptimeMillis()`: milliseconds since boot, EXCLUDING time
    /// spent in deep sleep — unlike `elapsed_realtime_ms` above. Feeds only
    /// `lifecycle::tick`'s suspend evidence (CORE-002).
    fn uptime_millis_ms(&self) -> CoreResult<i64> {
        self.java_vm
            .attach_current_thread(|env| -> Result<i64, jni::errors::Error> {
                env.call_static_method(
                    jni_str!("android/os/SystemClock"),
                    jni_str!("uptimeMillis"),
                    jni_sig!("()J"),
                    &[],
                )?
                .j()
            })
            .map_err(|err| CoreError::CommandFailed(format!("uptimeMillis failed: {err}")))
    }
}

impl LifecycleHooks for AndroidPlatformHooks {
    fn get_monotonic_clock_ms(&self) -> CoreResult<i64> {
        self.uptime_millis_ms()
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

    // Android gives a foreground service no reliable last-alive record — a
    // logout is never observed here, an accepted gap rather than a false
    // negative being papered over.
    fn get_last_logout_utc_ms(&self) -> CoreResult<Option<i64>> {
        Ok(None)
    }
}

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

        // jni's catch_unwind (see `native_result` below) collapses every panic
        // into "Rust panic: non-string panic payload" with no detail once it
        // crosses into the Java exception — this is the only place the real
        // message/location survives, so it needs to be captured here.
        std::panic::set_hook(Box::new(|info| {
            tracing::error!(panic = %info, "daemon thread panicked");
        }));

        guard
    });
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeInit<'l>(
    mut unowned_env: EnvUnowned<'l>,
    _class: JClass<'l>,
    config_dir: JString<'l>,
    data_dir: JString<'l>,
) -> jstring {
    native_result(&mut unowned_env, |env| -> Result<()> {
        let config_dir: String = config_dir.to_string();
        let data_dir: String = data_dir.to_string();

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
                .find_class(jni_str!("org/virtueinitiative/virtue/ScreenshotService"))
                .context("failed to find ScreenshotService class")?;
            let screenshot_service_class = Arc::new(
                env.new_global_ref(class)
                    .context("failed to create a global ref for ScreenshotService")?,
            );

            // Register the OCR callback (once; OnceLock ignores repeat calls).
            // The class is cached here on the main thread; background threads use the
            // global ref so the app class loader is not needed at call time.
            let virtue_ocr_class = env
                .find_class(jni_str!("org/virtueinitiative/virtue/VirtueOcr"))
                .context("failed to find VirtueOcr class")?;
            let virtue_ocr_global = Arc::new(
                env.new_global_ref(virtue_ocr_class)
                    .context("failed to create a global ref for VirtueOcr")?,
            );
            let vm_for_ocr = java_vm.clone();
            virtue_text_detection::android::register_recognize_fn(move |image, language| {
                vm_for_ocr
                    .attach_current_thread(|ocr_env| -> Result<String, CallError> {
                        let j_bytes = ocr_env.byte_array_from_slice(image)?;
                        let j_lang = JString::new(ocr_env, language.unwrap_or(""))?;
                        let j_str_obj = ocr_env
                            .call_static_method(
                                &*virtue_ocr_global,
                                jni_str!("recognizeText"),
                                jni_sig!("([BLjava/lang/String;)Ljava/lang/String;"),
                                &[JValue::Object(&j_bytes), JValue::Object(&j_lang)],
                            )?
                            .l()?;
                        Ok(ocr_env.cast_local::<JString>(j_str_obj)?.to_string())
                    })
                    .map_err(|err| OcrError::Recognition(err.to_string()))
            });

            let state_dir = PathBuf::from(&data_dir);
            let config = build_core_config(&state_dir);
            let platform = AndroidPlatformHooks {
                java_vm,
                screenshot_service_class,
            };
            let state_path = state_dir.join("event_state.json");
            let api = HttpApiClient::new(&config)?;
            let daemon = Daemon::new(config, platform, api, state_path)
                .map_err(|err| anyhow!("failed to construct daemon: {err}"))?;

            CORE.set(AndroidCore {
                state_dir,
                daemon: Arc::new(daemon),
                daemon_running: Mutex::new(false),
            })
            .map_err(|_| anyhow!("core already initialized"))?;
        }

        Ok(())
    })
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeLogin<'l>(
    mut unowned_env: EnvUnowned<'l>,
    _class: JClass<'l>,
    email: JString<'l>,
    password: JString<'l>,
    device_name: JString<'l>,
) -> jstring {
    native_result(&mut unowned_env, |_env| -> Result<()> {
        let email: String = email.to_string();
        let password: String = password.to_string();
        let device_name: String = device_name.to_string();
        let core = core()?;
        core.daemon
            .login(&email, &password, Some(&device_name))
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(())
    })
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeLogout<'l>(
    mut unowned_env: EnvUnowned<'l>,
    _class: JClass<'l>,
) -> jstring {
    native_result(&mut unowned_env, |_env| -> Result<()> {
        core()?
            .daemon
            .logout()
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(())
    })
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeIsLoggedIn<'l>(
    _env: EnvUnowned<'l>,
    _class: JClass<'l>,
) -> jboolean {
    matches!(
        core()
            .ok()
            .and_then(|core| read_auth_state(&core.state_dir))
            .map(|auth| auth.device_credentials.is_some()),
        Some(true)
    )
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeGetDeviceId<'l>(
    mut unowned_env: EnvUnowned<'l>,
    _class: JClass<'l>,
) -> jstring {
    native_string(&mut unowned_env, |_env| {
        core()
            .ok()
            .and_then(|core| read_auth_state(&core.state_dir))
            .and_then(|auth| auth.device_credentials.map(|device| device.device_id))
    })
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeRunDaemonLoop<'l>(
    mut unowned_env: EnvUnowned<'l>,
    _class: JClass<'l>,
) -> jstring {
    native_result(&mut unowned_env, |_env| -> Result<()> {
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

        // `Daemon::new` (in `nativeInit`) already does this once, but the
        // accessibility service can stop and resume this same loop many
        // times within one process without a fresh `nativeInit` in between
        // — each resume needs its own `note_user_start` to clear a prior
        // `note_user_stop`, or tamper detection would stay suspended for
        // the rest of the process's life. A no-op when not currently
        // stopped. See CORE-002.
        core.daemon.note_user_start();

        core.daemon.run_forever();

        if let Ok(mut guard) = core.daemon_running.lock() {
            *guard = false;
        }
        Ok(())
    })
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeStopDaemon<'l>(
    mut unowned_env: EnvUnowned<'l>,
    _class: JClass<'l>,
) -> jstring {
    native_result(&mut unowned_env, |_env| -> Result<()> {
        core()?.daemon.request_stop();
        Ok(())
    })
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeNoteUserStop<'l>(
    mut unowned_env: EnvUnowned<'l>,
    _class: JClass<'l>,
    source: JString<'l>,
) -> jstring {
    native_result(&mut unowned_env, |_env| -> Result<()> {
        let core = core()?;
        let source: String = source.to_string();
        core.daemon.note_user_stop(&source);
        Ok(())
    })
}

#[no_mangle]
pub extern "system" fn Java_org_virtueinitiative_virtue_NativeBridge_nativeGetStatusJson<'l>(
    mut unowned_env: EnvUnowned<'l>,
    _class: JClass<'l>,
) -> jstring {
    native_string(&mut unowned_env, |_env| {
        Some(
            (|| -> Result<String> {
                let core = core()?;
                Ok(serde_json::to_string(&core.daemon.status())?)
            })()
            .unwrap_or_else(|_| "{}".to_string()),
        )
    })
}

pub fn is_interactive(env: &mut Env) -> jni::errors::Result<bool> {
    let activity_thread = env
        .call_static_method(
            jni_str!("android/app/ActivityThread"),
            jni_str!("currentApplication"),
            jni_sig!("()Landroid/app/Application;"),
            &[],
        )?
        .l()?;

    let power_service = env
        .get_static_field(
            jni_str!("android/content/Context"),
            jni_str!("POWER_SERVICE"),
            jni_sig!("Ljava/lang/String;"),
        )?
        .l()?;

    let power_manager = env
        .call_method(
            &activity_thread,
            jni_str!("getSystemService"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
            &[JValue::Object(&power_service)],
        )?
        .l()?;

    let interactive = env
        .call_method(
            &power_manager,
            jni_str!("isInteractive"),
            jni_sig!("()Z"),
            &[],
        )?
        .z()?;

    Ok(interactive)
}

fn build_core_config(state_dir: &Path) -> Config {
    // The device name passed here is only a placeholder: device registration
    // happens on login, which carries the user-chosen name explicitly.
    Config::new(
        DEFAULT_BASE_API_URL,
        "android",
        "android",
        state_dir.to_path_buf(),
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

fn to_jstring_result(env: &mut Env, result: Result<()>) -> jstring {
    match result {
        Ok(()) => std::ptr::null_mut(),
        Err(err) => JString::new(env, err.to_string())
            .map(|value| value.into_raw())
            .unwrap_or(std::ptr::null_mut()),
    }
}

/// Shared body for the native methods that follow the "null on success, error
/// message otherwise" convention. `with_env` upgrades the raw JNI pointer and
/// wraps `body` in `catch_unwind`, so a panic throws a Java exception instead
/// of unwinding across the FFI boundary (which was undefined behaviour under
/// the previous jni 0.21 signatures).
fn native_result<F>(unowned: &mut EnvUnowned<'_>, body: F) -> jstring
where
    F: FnOnce(&mut Env) -> Result<()>,
{
    unowned
        .with_env(|env| -> std::result::Result<jstring, jni::errors::Error> {
            let result = body(env);
            Ok(to_jstring_result(env, result))
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// Same, for the methods that return a value rather than an error string.
fn native_string<F>(unowned: &mut EnvUnowned<'_>, body: F) -> jstring
where
    F: FnOnce(&mut Env) -> Option<String>,
{
    unowned
        .with_env(|env| -> std::result::Result<jstring, jni::errors::Error> {
            Ok(match body(env) {
                Some(value) => JString::new(env, value)
                    .map(|value| value.into_raw())
                    .unwrap_or(std::ptr::null_mut()),
                None => std::ptr::null_mut(),
            })
        })
        .resolve::<ThrowRuntimeExAndDefault>()
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
