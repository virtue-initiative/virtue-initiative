use std::ffi::{CStr, CString};
use std::fs;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use once_cell::sync::OnceCell;
use serde::de::DeserializeOwned;
use virtue_core::api::{BugReportRequest, HttpApiClient};
use virtue_core::{
    AuthState, Config, CoreError, CoreResult, Daemon, DeviceSettings, LifecycleHooks, Screenshot,
    ScreenshotHooks,
};

static CORE: OnceCell<IosCore> = OnceCell::new();
// Kept alive for the process lifetime; dropping it would silently stop the
// background thread that flushes buffered log lines. A dedicated `OnceCell`
// (rather than piggybacking on `CORE`, which has no room to hold a guard)
// still only ever runs its init closure once per process.
static LOG_GUARD: OnceCell<tracing_appender::non_blocking::WorkerGuard> = OnceCell::new();

const DEFAULT_BASE_API_URL: &str = virtue_core::DEFAULT_API_BASE_URL;

const CAPTURE_STATUS_READY: c_int = 0;
const CAPTURE_STATUS_PERMISSION_MISSING: c_int = 1;
const CAPTURE_STATUS_SESSION_UNAVAILABLE: c_int = 2;
const CAPTURE_STATUS_UNKNOWN: c_int = 3;

unsafe extern "C" {
    fn virtue_ios_capture_status() -> c_int;
    fn virtue_ios_capture_png_write(out_ptr: *mut *const u8, out_len: *mut usize) -> c_int;
    fn virtue_ios_capture_png_release(ptr: *const u8, len: usize);
    /// Distinguishes the app target (always `false` — `CaptureCallbacks.swift`)
    /// from the Safari extension target (always `true` —
    /// `SafariWebExtensionHandler.swift`), since both targets link this same
    /// Rust code but only the extension process has a real frame source. See
    /// `ScreenshotHooks::can_force_capture_now`.
    fn virtue_ios_can_force_capture() -> bool;
}

type IosDaemon = Daemon<IosPlatformHooks, HttpApiClient>;

struct IosCore {
    state_dir: PathBuf,
    daemon: Arc<IosDaemon>,
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

    fn can_force_capture_now(&self) -> bool {
        unsafe { virtue_ios_can_force_capture() }
    }
}

// iOS has no boot/shutdown/session API surface available to a Safari
// extension host, and `lifecycle_enabled()` returning `false` means
// `Daemon::tick_once` never calls `lifecycle::tick` at all — these two
// getters exist only to satisfy the trait and are never read.
impl LifecycleHooks for IosPlatformHooks {
    fn get_last_login_utc_ms(&self) -> CoreResult<Option<i64>> {
        Ok(None)
    }

    fn get_last_logout_utc_ms(&self) -> CoreResult<Option<i64>> {
        Ok(None)
    }

    fn lifecycle_enabled(&self) -> bool {
        false
    }
}

/// Installs the process-wide `tracing` subscriber on first call, writing
/// daily-rotated plain-text logs to `<data_dir>/logs/virtue.<date>.log`.
/// Subsequent calls are no-ops. No runtime override (no shell env vars on
/// mobile) — the compiled-in default filter for the build type is used
/// directly.
///
/// Uses the same `Builder` with an explicit `.log` filename suffix that
/// Mac/Windows use (the bare `rolling::daily` constructor leaves the
/// filename extensionless, e.g. `virtue.2026-08-22`) — `recent_logs` below
/// depends on that suffix to find today's/yesterday's files.
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

        let file_appender = tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix(virtue_core::logging::DEFAULT_FILE_LOG_POLICY.file_name_prefix)
            .filename_suffix("log")
            .build(&log_dir);

        let filter = tracing_subscriber::EnvFilter::new(
            virtue_core::logging::default_filter_directive(cfg!(debug_assertions)),
        );

        match file_appender {
            Ok(file_appender) => {
                let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
                tracing_subscriber::fmt()
                    .with_env_filter(filter)
                    .with_writer(non_blocking)
                    .with_ansi(false)
                    .init();
                guard
            }
            Err(err) => {
                eprintln!("failed to open log file in {}: {err}", log_dir.display());
                let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stderr());
                tracing_subscriber::fmt()
                    .with_env_filter(filter)
                    .with_writer(non_blocking)
                    .with_ansi(false)
                    .init();
                guard
            }
        }
    });
}

static DEFAULT_API_BASE_URL_C: OnceCell<CString> = OnceCell::new();

/// Returns a pointer valid for the process lifetime (backed by a `OnceCell`-held
/// `CString`) — unlike the other string-returning exports here, the caller must
/// NOT pass this to `virtue_ios_free_string`.
#[no_mangle]
pub extern "C" fn virtue_ios_default_api_base_url() -> *const c_char {
    DEFAULT_API_BASE_URL_C
        .get_or_init(|| CString::new(DEFAULT_BASE_API_URL).expect("no NUL bytes in URL"))
        .as_ptr()
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

            let state_dir = PathBuf::from(&data_dir);
            let config = build_core_config(&state_dir);
            let state_path = state_dir.join("event_state.json");
            let api = HttpApiClient::new(&config)?;
            let daemon = Daemon::new(config, IosPlatformHooks, api, state_path)
                .map_err(|err| anyhow!("failed to construct daemon: {err}"))?;

            CORE.set(IosCore {
                state_dir,
                daemon: Arc::new(daemon),
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
        core.daemon
            .login(&email, &password, Some(&device_name))
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(())
    })();

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_logout() -> *mut c_char {
    let result = (|| -> Result<()> {
        core()?
            .daemon
            .logout()
            .map_err(|err| anyhow!(err.to_string()))?;
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

/// Submits `POST /bug-report` (API-042). `contact_email` is treated as unset
/// when blank. `platform_details` is gathered by the Swift side (via
/// `UIDevice`) and passed straight through, mirroring the Android client's
/// Kotlin-side `platformDetails` parameter — unlike Mac/Windows, there's no
/// natural place to shell out for OS version info from this crate. Reads the
/// device's refresh token straight off disk, same disk-fallback approach
/// every other platform's `report-issue` flow uses, so a report can be
/// attributed to this device even if it isn't currently signed in to a fresh
/// `Daemon`; when `include_logs` is true, reads/redacts/trims the last two
/// days of this device's own rotated log files (`<state_dir>/logs`) for the
/// optional attachment.
#[no_mangle]
pub extern "C" fn virtue_ios_native_report_issue(
    message: *const c_char,
    contact_email: *const c_char,
    include_logs: bool,
    platform_details: *const c_char,
) -> *mut c_char {
    let result = (|| -> Result<()> {
        let core = core()?;
        let message = c_string_or_empty(message).trim().to_string();
        if message.is_empty() {
            return Err(anyhow!("message is required"));
        }
        let contact_email = c_string_or_empty(contact_email);
        let contact_email =
            (!contact_email.trim().is_empty()).then(|| contact_email.trim().to_string());
        let platform_details = c_string_or_empty(platform_details);

        let bearer_token = read_auth_state(&core.state_dir)
            .device_credentials
            .map(|creds| creds.refresh_token);

        let logs = include_logs.then(|| recent_logs(&core.state_dir)).flatten();

        let config = build_core_config(&core.state_dir);
        let api = HttpApiClient::new(&config)?;
        api.report_issue(
            bearer_token.as_deref(),
            &BugReportRequest {
                message: &message,
                contact_email: contact_email.as_deref(),
                platform: "ios",
                app_version: virtue_core::BUILD_LABEL,
                platform_details: Some(&platform_details),
            },
            logs.as_deref(),
        )
        .context("failed to submit bug report")?;
        Ok(())
    })();
    into_c_result(result)
}

/// Best-effort last two days of this device's own logs: today's and (if
/// present) yesterday's daily-rotated log file from `<state_dir>/logs` (see
/// `init_logging`), redacted (`virtue_core::api::redact_secrets`) and
/// trimmed to the API's attachment size cap, keeping the most recent bytes.
fn recent_logs(state_dir: &Path) -> Option<Vec<u8>> {
    let log_dir = state_dir.join("logs");
    let today = chrono::Local::now().date_naive();
    let mut combined = String::new();

    for date in [today, today - chrono::Duration::days(1)] {
        let file_name = format!(
            "{}.{}.log",
            virtue_core::logging::DEFAULT_FILE_LOG_POLICY.file_name_prefix,
            date.format("%Y-%m-%d")
        );
        if let Ok(contents) = fs::read_to_string(log_dir.join(file_name)) {
            combined.push_str(&contents);
        }
    }

    if combined.is_empty() {
        return None;
    }

    let redacted = virtue_core::api::redact_secrets(&combined);
    let mut logs = redacted.into_bytes();
    if logs.len() > virtue_core::api::MAX_LOG_ATTACHMENT_BYTES {
        let start = logs.len() - virtue_core::api::MAX_LOG_ATTACHMENT_BYTES;
        logs.drain(0..start);
    }
    Some(logs)
}

/// Returns a JSON-serialized `ServiceStatus` (caller frees with
/// `virtue_ios_free_string`), or null on failure.
#[no_mangle]
pub extern "C" fn virtue_ios_native_get_status_json() -> *mut c_char {
    let json = (|| -> Result<String> {
        let core = core()?;
        Ok(serde_json::to_string(&core.daemon.status())?)
    })();

    match json {
        Ok(value) => CString::new(value)
            .map(CString::into_raw)
            .unwrap_or(std::ptr::null_mut()),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Queues a one-shot forced capture, serviced by the next tick run by a
/// process that reports `can_force_capture_now() == true` — on iOS, that's
/// specifically the Safari extension process, not this call's caller (the
/// main app). No daemon loop needs to be running in this process for this
/// call to succeed, unlike `login`/`logout`/etc.
#[no_mangle]
pub extern "C" fn virtue_ios_native_request_force_capture() -> *mut c_char {
    let result = (|| -> Result<()> {
        core()?.daemon.request_forced_capture();
        Ok(())
    })();

    into_c_result(result)
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

        core.daemon.run_forever();

        if let Ok(mut guard) = core.daemon_running.lock() {
            *guard = false;
        }
        Ok(())
    })();

    into_c_result(result)
}

/// Applies any pending requests and runs exactly one tick, then returns —
/// see `Daemon::tick_once` / CORE-015. This is what the Safari
/// extension's native message handler calls (synchronously, once per
/// `beginRequest`) instead of `virtue_ios_native_run_daemon_loop`: the OS
/// only guarantees that process runs for the duration of one message's
/// round trip, not long enough for a background loop thread to do anything
/// useful.
#[no_mangle]
pub extern "C" fn virtue_ios_native_tick_once() -> *mut c_char {
    let result = (|| -> Result<()> {
        core()?.daemon.tick_once();
        Ok(())
    })();

    into_c_result(result)
}

/// Process-lifetime count of NSFW model invocations, for the same
/// memory-diagnostics purpose as `virtue_ios_native_batch_upload_count`.
#[no_mangle]
pub extern "C" fn virtue_ios_native_nsfw_run_count() -> u64 {
    virtue_core::module::screenshot::risk_classifier::nsfw_model_invocation_count()
}

/// Process-lifetime count of successful batch uploads, surfaced to
/// `background.js`'s console via `ProcessDiagnostics` so memory trends can be
/// correlated with capture/classify/upload activity without needing the Rust
/// file logs.
#[no_mangle]
pub extern "C" fn virtue_ios_native_batch_upload_count() -> u64 {
    virtue_core::module::upload::batch_upload_count()
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_stop_daemon() -> *mut c_char {
    let result = (|| -> Result<()> {
        core()?.daemon.request_stop();
        Ok(())
    })();

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_pause_monitoring(_source: *const c_char) -> *mut c_char {
    let result = (|| -> Result<()> {
        core()?.daemon.request_stop();
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
        core.daemon.note_user_stop(&source);
        Ok(())
    })();

    into_c_result(result)
}

#[no_mangle]
pub extern "C" fn virtue_ios_native_request_resume_monitoring() -> *mut c_char {
    let result = (|| -> Result<()> {
        core()?.daemon.note_user_start();
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

fn build_core_config(state_dir: &Path) -> Config {
    // The device name passed here is only a placeholder: device registration
    // happens on login, which carries the user-chosen name explicitly.
    Config::new(
        DEFAULT_BASE_API_URL,
        "ios",
        "ios",
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
        Err(err) => CString::new(format!("{err:#}"))
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
