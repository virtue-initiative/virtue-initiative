use std::ffi::{CStr, CString};
use std::fs;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use once_cell::sync::OnceCell;
use virtue_core::storage::FileStateStore;
use virtue_core::{Config, CoreError, CoreResult, MonitorService, PlatformHooks, Screenshot};

static CORE: OnceCell<IosCore> = OnceCell::new();

const DEFAULT_BASE_API_URL: &str = "https://api.virtueinitiative.org";
const DEFAULT_CAPTURE_INTERVAL_SECONDS: u64 = 300;
const DEFAULT_BATCH_WINDOW_SECONDS: u64 = 3600;
const ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(20);

const CAPTURE_STATUS_READY: c_int = 0;
const CAPTURE_STATUS_PERMISSION_MISSING: c_int = 1;
const CAPTURE_STATUS_SESSION_UNAVAILABLE: c_int = 2;

unsafe extern "C" {
    fn virtue_ios_capture_status() -> c_int;
    fn virtue_ios_capture_png_write(out_ptr: *mut *const u8, out_len: *mut usize) -> c_int;
    fn virtue_ios_capture_png_release(ptr: *const u8, len: usize);
}

struct IosCore {
    state_dir: PathBuf,
    runtime_config_file: PathBuf,
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

impl PlatformHooks for IosPlatformHooks {
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
            CORE.set(IosCore {
                state_dir: PathBuf::from(data_dir),
                runtime_config_file,
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
pub extern "C" fn virtue_ios_native_set_overrides(
    base_api_url: *const c_char,
    capture_interval_seconds: *const c_char,
    batch_window_seconds: *const c_char,
) -> *mut c_char {
    let result = (|| -> Result<()> {
        let core = core()?;
        let base_api_url = c_string_or_empty(base_api_url);
        let capture_interval_seconds = c_string_or_empty(capture_interval_seconds);
        let batch_window_seconds = c_string_or_empty(batch_window_seconds);

        write_runtime_overrides(
            &core.runtime_config_file,
            &base_api_url,
            &capture_interval_seconds,
            &batch_window_seconds,
        )
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

        let mut service =
            MonitorService::setup(build_core_config(core, &device_name), IosPlatformHooks)?;
        service.login(&email, &password)?;
        Ok(())
    })();

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_logout() -> *mut c_char {
    let result = (|| -> Result<()> {
        let core = core()?;
        let mut service =
            MonitorService::setup(build_core_config(core, "ios-device"), IosPlatformHooks)?;
        service.logout()?;
        Ok(())
    })();

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_is_logged_in() -> bool {
    core()
        .and_then(|core| Ok(FileStateStore::new(&core.state_dir)?.load_auth_state()?))
        .map(|auth| auth.device_credentials.is_some())
        .unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_get_device_id() -> *mut c_char {
    let device_id = core()
        .and_then(|core| Ok(FileStateStore::new(&core.state_dir)?.load_auth_state()?))
        .ok()
        .and_then(|auth| auth.device_credentials.map(|device| device.device_id));

    match device_id {
        Some(value) => CString::new(value)
            .map(CString::into_raw)
            .unwrap_or(std::ptr::null_mut()),
        None => std::ptr::null_mut(),
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
pub extern "C" fn virtue_ios_free_string(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    unsafe {
        let _ = CString::from_raw(value);
    }
}

fn run_daemon_loop(core: &IosCore) -> Result<()> {
    let mut service =
        MonitorService::setup(build_core_config(core, "ios-device"), IosPlatformHooks)?;

    while !core.stop.load(Ordering::SeqCst) {
        let sleep_duration = match service.loop_iteration() {
            Ok(outcome) => duration_until(outcome.next_run_at_ms),
            Err(err) => {
                eprintln!("ios-daemon: {err}");
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

fn build_core_config(core: &IosCore, device_name: &str) -> Config {
    Config::new(
        DEFAULT_BASE_API_URL,
        device_name,
        "ios",
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
