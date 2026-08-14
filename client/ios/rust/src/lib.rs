use std::ffi::{CStr, CString};
use std::fs;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use once_cell::sync::OnceCell;
use serde::de::DeserializeOwned;
use virtue_core::{
    build_default_modules_reqwest, load_state, store_state, AuthState, Config, CoreError,
    CoreResult, DeviceSettings, EventBus, EventChannel, LifecycleHooks, LoginRequested,
    LoginResult, LogoutRequested, Ping, PlatformConfig, PlatformHooks, ProcessStarted,
    ProcessStopped, Redacted, Screenshot, ScreenshotHooks, StatusRequest, StatusResponse,
    UserStopRequested,
};

static CORE: OnceCell<IosCore> = OnceCell::new();
// Kept alive for the process lifetime; dropping it would silently stop the
// background thread that flushes buffered log lines. A dedicated `OnceCell`
// (rather than piggybacking on `CORE`, which has no room to hold a guard)
// still only ever runs its init closure once per process.
static LOG_GUARD: OnceCell<tracing_appender::non_blocking::WorkerGuard> = OnceCell::new();

const DEFAULT_BASE_API_URL: &str = virtue_core::DEFAULT_API_BASE_URL;
const ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(20);
// Ping every second (like the Android client), independent of capture cadence
// (governed separately by `capture_interval_seconds`). Lifecycle detection is
// disabled entirely on iOS (see `build_bus`), so this cadence is purely about
// keeping other modules (screenshot scheduling, upload batching) responsive.
const LOOP_INTERVAL: Duration = Duration::from_secs(1);

const CAPTURE_STATUS_READY: c_int = 0;
const CAPTURE_STATUS_PERMISSION_MISSING: c_int = 1;
const CAPTURE_STATUS_SESSION_UNAVAILABLE: c_int = 2;
const CAPTURE_STATUS_UNKNOWN: c_int = 3;

unsafe extern "C" {
    fn virtue_ios_capture_status() -> c_int;
    fn virtue_ios_capture_png_write(out_ptr: *mut *const u8, out_len: *mut usize) -> c_int;
    fn virtue_ios_capture_png_release(ptr: *const u8, len: usize);
}

struct IosCore {
    state_dir: PathBuf,
    stop: AtomicBool,
    daemon_running: Mutex<bool>,
}

#[derive(Clone)]
struct IosPlatformHooks;

impl IosPlatformHooks {
    fn capture_status(&self) -> c_int {
        unsafe { virtue_ios_capture_status() }
    }

    fn capture_png(&self) -> Result<Vec<u8>, CoreError> {
        let mut ptr: *const u8 = std::ptr::null();
        let mut len: usize = 0;

        let rc = unsafe { virtue_ios_capture_png_write(&mut ptr, &mut len) };
        if rc != 0 {
            return Err(CoreError::CommandFailed(format!(
                "capture callback returned error code {rc}"
            )));
        }
        if ptr.is_null() || len == 0 {
            return Err(CoreError::CommandFailed(
                "capture callback returned empty frame".to_string(),
            ));
        }

        let bytes = unsafe { std::slice::from_raw_parts(ptr, len).to_vec() };
        unsafe { virtue_ios_capture_png_release(ptr, len) };
        Ok(bytes)
    }
}

impl ScreenshotHooks for IosPlatformHooks {
    fn take_screenshot(&self) -> CoreResult<Screenshot> {
        match self.capture_status() {
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
            CAPTURE_STATUS_UNKNOWN => Err(CoreError::CommandFailed(
                "capture status unknown".to_string(),
            )),
            other => Err(CoreError::CommandFailed(format!(
                "unexpected capture status code: {other}"
            ))),
        }
    }

}

// Inert: iOS has no boot/shutdown/session API surface available to a Safari
// extension host, and `PlatformConfig { lifecycle_enabled: false }` (set in
// `build_bus`) means `LifecycleModule` is never constructed here — a
// `NoopLifecycleModule` stands in instead. These methods only exist to
// satisfy `PlatformHooks: ScreenshotHooks + LifecycleHooks`'s trait bound on
// `build_default_modules_reqwest` and are never called.
impl LifecycleHooks for IosPlatformHooks {
    fn get_boot_clock_ms(&self) -> CoreResult<i64> {
        Ok(0)
    }

    fn get_monotonic_clock_ms(&self) -> CoreResult<i64> {
        Ok(0)
    }

    fn get_last_login_utc_ms(&self) -> CoreResult<Option<i64>> {
        Ok(None)
    }

    fn get_last_logout_utc_ms(&self) -> CoreResult<Option<i64>> {
        Ok(None)
    }
}

impl PlatformHooks for IosPlatformHooks {}

/// Installs the process-wide `tracing` subscriber on first call, writing
/// daily-rotated plain-text logs to `<data_dir>/logs/virtue.log`. Subsequent
/// calls are no-ops. No runtime override (no shell env vars on mobile) — the
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
pub extern "C" fn virtue_ios_default_capture_interval_seconds() -> u64 {
    virtue_core::default_capture_interval_seconds()
}

#[no_mangle]
pub extern "C" fn virtue_ios_default_batch_window_seconds() -> u64 {
    virtue_core::default_batch_window_seconds()
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_init(
    config_dir: *const c_char,
    data_dir: *const c_char,
) -> *mut c_char {
    let result = (|| -> Result<()> {
        let config_dir = c_string_or_empty(config_dir);
        let data_dir = c_string_or_empty(data_dir);

        fs::create_dir_all(&config_dir)
            .with_context(|| format!("failed to create config dir {config_dir}"))?;
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("failed to create data dir {data_dir}"))?;
        sanitize_state_dir(Path::new(&data_dir))?;

        if CORE.get().is_none() {
            init_logging(Path::new(&data_dir));

            CORE.set(IosCore {
                state_dir: PathBuf::from(data_dir),
                stop: AtomicBool::new(false),
                daemon_running: Mutex::new(false),
            })
            .map_err(|_| anyhow!("core already initialized"))?;
        }

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
        let email = c_string_or_empty(email);
        let password = c_string_or_empty(password);
        let device_name = c_string_or_empty(device_name);
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

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_logout() -> *mut c_char {
    let result = (|| -> Result<()> {
        let core = core()?;
        let (mut bus, state_path) = build_bus(core)?;
        bus.send(LogoutRequested)?;
        let state = bus.iter()?;
        store_state(&state_path, &state)?;
        Ok(())
    })();

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_is_logged_in() -> bool {
    core()
        .map(|core| {
            read_auth_state(&core.state_dir)
                .device_credentials
                .is_some()
        })
        .unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_get_device_id() -> *mut c_char {
    let device_id = core().ok().and_then(|core| {
        read_auth_state(&core.state_dir)
            .device_credentials
            .map(|d| d.device_id)
    });

    match device_id {
        Some(value) => CString::new(value)
            .map(CString::into_raw)
            .unwrap_or(std::ptr::null_mut()),
        None => std::ptr::null_mut(),
    }
}

/// Build a transient bus from the shared event state and ask it for a status
/// snapshot. Returns a JSON-serialized `ServiceStatus` (caller frees with
/// `virtue_ios_free_string`), or null on failure.
///
/// Status is no longer published to a `status.json` file by the core; it is
/// produced on demand via a `StatusRequest`/`StatusResponse` round-trip on the
/// event bus, mirroring the Android client.
#[no_mangle]
pub extern "C" fn virtue_ios_native_get_status_json() -> *mut c_char {
    let json = (|| -> Result<String> {
        let core = core()?;
        let (mut bus, _) = build_bus(core)?;
        let response = bus.request::<StatusRequest, StatusResponse>(StatusRequest)?;
        Ok(serde_json::to_string(&response.status)?)
    })();

    match json {
        Ok(value) => CString::new(value)
            .map(CString::into_raw)
            .unwrap_or(std::ptr::null_mut()),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_run_daemon_loop() -> *mut c_char {
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

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_stop_daemon() -> *mut c_char {
    let result = (|| -> Result<()> {
        let core = core()?;
        core.stop.store(true, Ordering::SeqCst);
        Ok(())
    })();

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_pause_monitoring(source: *const c_char) -> *mut c_char {
    let result = (|| -> Result<()> {
        let core = core()?;
        let _source = match c_string_or_empty(source).trim() {
            "" => "ios_pause_button".to_string(),
            value => value.to_string(),
        };
        core.stop.store(true, Ordering::SeqCst);
        Ok(())
    })();

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_request_pause_monitoring(source: *const c_char) -> *mut c_char {
    let result = (|| -> Result<()> {
        let core = core()?;
        let source = match c_string_or_empty(source).trim() {
            "" => "ios_pause_button".to_string(),
            value => value.to_string(),
        };
        let (mut bus, state_path) = build_bus(core)?;
        bus.send(UserStopRequested { source })?;
        let state = bus.iter()?;
        store_state(&state_path, &state)?;
        Ok(())
    })();

    into_c_result(result)
}

#[no_mangle]
/// # Safety
///
/// `value` must have been returned by this library via `CString::into_raw`
/// and must not be freed more than once.
pub unsafe extern "C" fn virtue_ios_free_string(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    let _ = unsafe { CString::from_raw(value) };
}

fn run_daemon_loop(core: &IosCore) -> Result<()> {
    let (mut bus, state_path) = build_bus(core)?;
    bus.send(ProcessStarted)?;
    // The screenshot module's `enabled` flag is set on `Login` and persisted in
    // `event_state.json`, so it survives across the separate login/daemon FFI calls
    // and is already `true` here once the user has logged in. iOS has no lock/screen
    // concept exposed to the daemon, so `is_locked_or_screensaver()` stays at the
    // default `Ok(false)` and capture proceeds whenever the loop runs.
    let state = bus.iter()?;
    store_state(&state_path, &state)?;

    while !core.stop.load(Ordering::SeqCst) {
        if let Err(err) = (|| -> Result<()> {
            bus.send(Ping)?;
            let state = bus.iter()?;
            store_state(&state_path, &state)?;
            Ok(())
        })() {
            tracing::error!(error = %err, "ios-daemon");
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

fn build_bus(core: &IosCore) -> Result<(EventBus, PathBuf)> {
    let cfg = build_core_config(core);
    // The Safari extension host has no `UIApplication` and can be suspended by
    // the OS the instant the device locks, with no notification delivered to
    // this process, and no boot/shutdown/session API surface at all. There's no
    // way to build a meaningful expected-running-window model here, so
    // lifecycle detection is disabled entirely — `assembly::build_default_modules`
    // constructs a `NoopLifecycleModule` instead of a real `LifecycleModule`.
    let platform_config = PlatformConfig {
        lifecycle_enabled: false,
    };
    let modules = build_default_modules_reqwest(cfg, IosPlatformHooks, platform_config)?;
    let state_path = core.state_dir.join("event_state.json");
    let bus = EventBus::new(modules, load_state(&state_path)?)?;
    Ok((bus, state_path))
}

fn build_core_config(core: &IosCore) -> Config {
    // The device name passed at construction is only a placeholder: device
    // registration happens on login, which carries the user-chosen name on the
    // `LoginRequested` event.
    Config::new(
        DEFAULT_BASE_API_URL,
        "ios",
        "ios",
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

fn read_auth_state(state_dir: &Path) -> AuthState {
    let path = state_dir.join("event_state.json");
    let Ok(bytes) = std::fs::read(&path) else {
        return AuthState::default();
    };
    let Ok(state) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return AuthState::default();
    };
    state
        .get("auth")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn core() -> Result<&'static IosCore> {
    CORE.get().ok_or_else(|| anyhow!("core not initialized"))
}

fn c_string_or_empty(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

fn into_c_result(result: Result<()>) -> *mut c_char {
    match result {
        Ok(()) => std::ptr::null_mut(),
        Err(err) => CString::new(err.to_string())
            .map(CString::into_raw)
            .unwrap_or(std::ptr::null_mut()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_logging_is_idempotent_across_repeated_calls() {
        // `virtue_ios_native_init` can legitimately be called more than once per
        // process, and its one-time-setup block guards on `CORE.get().is_none()`
        // before calling `init_logging`. `init_logging` itself must also tolerate
        // more than one call without panicking, since nothing prevents it being
        // reached twice before `CORE` is set on a slow init path.
        let dir =
            std::env::temp_dir().join(format!("virtue-ios-logging-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        init_logging(&dir);
        init_logging(&dir);

        assert!(LOG_GUARD.get().is_some(), "logging should be initialized");

        std::fs::remove_dir_all(&dir).ok();
    }
}
