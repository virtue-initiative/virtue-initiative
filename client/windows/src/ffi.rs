use std::ffi::{CStr, CString, c_char};
use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use virtue_core::module::auth::CodeLoginPoll;

use crate::config::{ClientPaths, build_core_config, default_device_name};
use crate::resident_monitor::{self, MonitorStatusSnapshot};
use crate::session::{SessionManager, SessionStatus};

fn current_paths() -> Result<ClientPaths> {
    ClientPaths::discover()
}

// Keeps the non-blocking writer's flush thread alive for the process
// lifetime; dropping it would silently stop log writes. `OnceLock` makes the
// install idempotent — `virtue_windows_init` can be (and is, per its own
// tests) called more than once per process.
static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Installs the process-wide `tracing` subscriber on first call, writing
/// daily-rotated plain-text logs to `<data>\logs\virtue.<date>.log`. Subsequent
/// calls are no-ops — safe to call from every `virtue_windows_init` invocation.
fn init_logging(paths: &ClientPaths) {
    LOG_GUARD.get_or_init(|| {
        if let Err(err) = std::fs::create_dir_all(&paths.log_dir) {
            eprintln!(
                "failed to create logs dir {}: {err}",
                paths.log_dir.display()
            );
        }
        if let Err(err) = virtue_core::logging::prune_old_logs(
            &paths.log_dir,
            &virtue_core::logging::DEFAULT_FILE_LOG_POLICY,
        ) {
            eprintln!("failed to prune old logs: {err}");
        }

        let file_appender = tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix(virtue_core::logging::DEFAULT_FILE_LOG_POLICY.file_name_prefix)
            .filename_suffix("log")
            .build(&paths.log_dir);

        let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(virtue_core::logging::default_filter_directive(
                cfg!(debug_assertions),
            ))
        });

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
                eprintln!(
                    "failed to open log file in {}: {err}",
                    paths.log_dir.display()
                );
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

fn c_string_or_empty(value: *const c_char) -> String {
    if value.is_null() {
        return String::new();
    }

    unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .trim()
        .to_string()
}

fn into_error_ptr(result: Result<()>) -> *mut c_char {
    match result {
        Ok(()) => std::ptr::null_mut(),
        Err(err) => into_owned_c_string(format!("{err:#}")),
    }
}

fn into_json_ptr<T: Serialize>(result: Result<T>) -> *mut c_char {
    match result.and_then(|value| serde_json::to_string(&value).context("failed serializing json"))
    {
        Ok(json) => into_owned_c_string(json),
        Err(err) => into_owned_c_string(format!("{err:#}")),
    }
}

fn into_owned_c_string(value: String) -> *mut c_char {
    CString::new(value)
        .expect("CString::new should not fail for generated payloads")
        .into_raw()
}

fn ensure_paths_initialized() -> Result<ClientPaths> {
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;
    Ok(paths)
}

fn with_session_manager() -> Result<SessionManager> {
    let paths = current_paths()?;
    paths.ensure_dirs()?;
    Ok(SessionManager { paths })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionStatusPayload {
    logged_in: bool,
    device_id: Option<String>,
    email: Option<String>,
    build_label: String,
}

impl From<SessionStatus> for SessionStatusPayload {
    fn from(value: SessionStatus) -> Self {
        Self {
            logged_in: value.logged_in,
            device_id: value.device_id,
            email: value.email,
            build_label: virtue_core::BUILD_LABEL.to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusErrorPayload {
    at_ms: i64,
    context: String,
    message: String,
}

/// The status page's data (CORE-010), plus the Windows-only monitor state and
/// log directory the WinUI dialog also shows. Written out field by field
/// rather than flattening `ServiceStatus`, so this stays one explicit
/// camelCase contract with the C# DTO — see the contract test below.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MonitorStatusPayload {
    state: String,
    logged_in: bool,
    account_email: Option<String>,
    device_id: Option<String>,
    device_name: Option<String>,
    partner_count: Option<usize>,
    pending_hash_count: usize,
    pending_batch_count: usize,
    pending_request_count: usize,
    last_loop_at_ms: Option<i64>,
    last_screenshot_attempt_at_ms: Option<i64>,
    last_screenshot_at_ms: Option<i64>,
    last_skip_reason: Option<String>,
    last_batch_at_ms: Option<i64>,
    recent_errors: Vec<StatusErrorPayload>,
    api_base_url: Option<String>,
    hash_base_url: Option<String>,
    capture_interval_seconds: Option<u64>,
    batch_window_seconds: Option<u64>,
    last_error: Option<String>,
    log_directory: Option<String>,
}

/// What a "Test Screenshot" run did, mirrored by the C# `ForceCapturePayload`
/// DTO. `outcome` is the stable code; `message` is the shared user-facing
/// wording from `virtue_core::force_capture`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ForceCapturePayload {
    outcome: String,
    message: String,
}

fn skip_reason_label(reason: &virtue_core::StatusSkipReason) -> &'static str {
    match reason {
        virtue_core::StatusSkipReason::StaticScreen => "Screen unchanged since the last upload",
        virtue_core::StatusSkipReason::LockedOrScreensaver => "Screen locked or screensaver active",
        virtue_core::StatusSkipReason::CaptureFailed => "Capture failed",
    }
}

impl From<MonitorStatusSnapshot> for MonitorStatusPayload {
    fn from(value: MonitorStatusSnapshot) -> Self {
        let core = value.core;
        Self {
            state: value.state,
            logged_in: value.logged_in,
            account_email: core.as_ref().and_then(|s| s.account_email.clone()),
            device_id: core.as_ref().and_then(|s| s.device_id.clone()),
            device_name: core.as_ref().and_then(|s| s.device_name.clone()),
            partner_count: core.as_ref().and_then(|s| s.partner_count),
            pending_hash_count: core.as_ref().map(|s| s.pending_hash_count).unwrap_or(0),
            pending_batch_count: core.as_ref().map(|s| s.pending_batch_count).unwrap_or(0),
            pending_request_count: value.pending_request_count,
            last_loop_at_ms: core.as_ref().and_then(|s| s.last_loop_at_ms),
            last_screenshot_attempt_at_ms: core
                .as_ref()
                .and_then(|s| s.last_screenshot_attempt_at_ms),
            last_screenshot_at_ms: core.as_ref().and_then(|s| s.last_screenshot_at_ms),
            last_skip_reason: core
                .as_ref()
                .and_then(|s| s.last_skip_reason.as_ref())
                .map(|reason| skip_reason_label(reason).to_string()),
            last_batch_at_ms: core.as_ref().and_then(|s| s.last_batch_at_ms),
            recent_errors: core
                .as_ref()
                .map(|s| {
                    s.recent_errors
                        .iter()
                        .map(|error| StatusErrorPayload {
                            at_ms: error.at_ms,
                            context: error.context.clone(),
                            message: error.message.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            api_base_url: core.as_ref().map(|s| s.api_base_url.clone()),
            hash_base_url: core.as_ref().and_then(|s| s.hash_base_url.clone()),
            capture_interval_seconds: core.as_ref().map(|s| s.capture_interval_seconds),
            batch_window_seconds: core.as_ref().map(|s| s.batch_window_seconds),
            last_error: value.last_error,
            log_directory: ClientPaths::discover()
                .ok()
                .map(|paths| paths.log_dir.display().to_string()),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LoginRequest {
    email: String,
    password: String,
    device_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct BeginCodeLoginRequest {
    device_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BeginCodeLoginPayload {
    user_code: String,
    expires_at_ms: i64,
    interval_seconds: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PollCodeLoginPayload {
    status: &'static str,
    device_id: Option<String>,
}

impl From<CodeLoginPoll> for PollCodeLoginPayload {
    fn from(value: CodeLoginPoll) -> Self {
        match value {
            CodeLoginPoll::Pending => Self {
                status: "pending",
                device_id: None,
            },
            CodeLoginPoll::Approved { device_id } => Self {
                status: "approved",
                device_id: Some(device_id),
            },
            CodeLoginPoll::Expired => Self {
                status: "expired",
                device_id: None,
            },
            CodeLoginPoll::Unavailable => Self {
                status: "unavailable",
                device_id: None,
            },
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ReportIssueRequest {
    message: String,
    contact_email: Option<String>,
    include_logs: bool,
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_init() -> *mut c_char {
    into_error_ptr((|| {
        let paths = ensure_paths_initialized()?;
        init_logging(&paths);
        Ok(())
    })())
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_get_session_status_json() -> *mut c_char {
    into_json_ptr((|| {
        let manager = with_session_manager()?;
        let status = manager.status()?;
        Ok(SessionStatusPayload::from(status))
    })())
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_login(request_json: *const c_char) -> *mut c_char {
    into_error_ptr((|| {
        let request_json = c_string_or_empty(request_json);
        let request: LoginRequest =
            serde_json::from_str(&request_json).context("failed parsing login request json")?;

        if request.email.trim().is_empty() {
            return Err(anyhow!("email is required"));
        }
        if request.password.is_empty() {
            return Err(anyhow!("password is required"));
        }

        let device_name = request
            .device_name
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(default_device_name);

        let manager = with_session_manager()?;
        manager.login_blocking(&request.email, &request.password, &device_name)?;
        Ok(())
    })())
}

/// CORE-020. Returns the code to display plus the polling interval, as JSON.
#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_begin_code_login(request_json: *const c_char) -> *mut c_char {
    into_json_ptr((|| {
        let request_json = c_string_or_empty(request_json);
        let request: BeginCodeLoginRequest = if request_json.trim().is_empty() {
            BeginCodeLoginRequest::default()
        } else {
            serde_json::from_str(&request_json)
                .context("failed parsing begin-code-login request json")?
        };

        let device_name = request
            .device_name
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(default_device_name);

        let manager = with_session_manager()?;
        let start = manager.begin_code_login_blocking(&device_name)?;
        Ok(BeginCodeLoginPayload {
            user_code: start.user_code,
            expires_at_ms: start.expires_at_ms,
            interval_seconds: start.interval_seconds,
        })
    })())
}

/// CORE-021. `status` is one of `pending`, `approved`, or `expired`.
#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_poll_code_login() -> *mut c_char {
    into_json_ptr((|| {
        let manager = with_session_manager()?;
        Ok(PollCodeLoginPayload::from(
            manager.poll_code_login_blocking()?,
        ))
    })())
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_logout() -> *mut c_char {
    into_error_ptr((|| {
        let manager = with_session_manager()?;
        manager.logout_blocking()?;
        Ok(())
    })())
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_report_issue(request_json: *const c_char) -> *mut c_char {
    into_error_ptr((|| {
        let request_json = c_string_or_empty(request_json);
        let request: ReportIssueRequest = serde_json::from_str(&request_json)
            .context("failed parsing report-issue request json")?;

        let message = request.message.trim().to_string();
        if message.is_empty() {
            return Err(anyhow!("message is required"));
        }

        let paths = ensure_paths_initialized()?;

        // Read the device refresh token straight off disk, same disk-fallback
        // approach the Linux `report-issue` command uses, so a report can be
        // attributed to this device even when the resident daemon isn't
        // running (or hasn't finished starting up yet).
        let state: virtue_core::DaemonState =
            virtue_core::load_state(&paths.state_dir.join("event_state.json")).unwrap_or_default();
        let bearer_token = state
            .auth
            .device_credentials
            .map(|creds| creds.refresh_token);

        let platform_details = windows_platform_details();
        let logs = request.include_logs.then(|| recent_logs(&paths)).flatten();

        let config = build_core_config(&paths);
        let api = virtue_core::api::HttpApiClient::new(&config)?;
        api.report_issue(
            bearer_token.as_deref(),
            &virtue_core::api::BugReportRequest {
                message: &message,
                contact_email: request.contact_email.as_deref(),
                platform: "windows",
                app_version: virtue_core::BUILD_LABEL,
                platform_details: Some(&platform_details),
            },
            logs.as_deref(),
        )
        .context("failed to submit bug report")?;

        Ok(())
    })())
}

/// Best-effort "ProductName; DisplayVersion (Build CurrentBuildNumber)" string
/// read from `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion`, e.g.
/// `"Windows 11 Pro; 23H2 (Build 22631)"`. Falls back to a fixed placeholder
/// on any registry error, mirroring the Linux client's `"unknown"` fallback
/// in `linux_platform_details`.
#[cfg(target_os = "windows")]
fn windows_platform_details() -> String {
    let product_name = read_registry_string_value(
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        "ProductName",
    );
    let display_version = read_registry_string_value(
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        "DisplayVersion",
    )
    .or_else(|| {
        read_registry_string_value(
            "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
            "ReleaseId",
        )
    });
    let build_number = read_registry_string_value(
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        "CurrentBuildNumber",
    );

    let mut parts = Vec::new();
    if let Some(product_name) = product_name {
        parts.push(product_name);
    }

    let version_part = match (display_version, build_number) {
        (Some(version), Some(build)) => Some(format!("{version} (Build {build})")),
        (Some(version), None) => Some(version),
        (None, Some(build)) => Some(format!("Build {build}")),
        (None, None) => None,
    };
    if let Some(version_part) = version_part {
        parts.push(version_part);
    }

    if parts.is_empty() {
        "Windows (unknown version)".to_string()
    } else {
        parts.join("; ")
    }
}

#[cfg(not(target_os = "windows"))]
fn windows_platform_details() -> String {
    "Windows (unknown version)".to_string()
}

/// Reads a single `REG_SZ` value under `HKLM\<key_path>`, or `None` on any
/// error (missing key/value, wrong type, non-UTF-16 data).
#[cfg(target_os = "windows")]
fn read_registry_string_value(key_path: &str, value_name: &str) -> Option<String> {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
    };
    use windows::core::PCWSTR;

    let key_path_wide: Vec<u16> = format!("{key_path}\0").encode_utf16().collect();
    let value_name_wide: Vec<u16> = format!("{value_name}\0").encode_utf16().collect();

    unsafe {
        let mut hkey = HKEY::default();
        let open_result = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR::from_raw(key_path_wide.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        );
        if open_result.is_err() {
            return None;
        }

        // Grow-and-retry: query the required size first, then fetch into a
        // buffer of exactly that size (values here are short, so one retry
        // at most is expected in practice).
        let mut data_size = 0u32;
        let size_result = RegQueryValueExW(
            hkey,
            PCWSTR::from_raw(value_name_wide.as_ptr()),
            None,
            None,
            None,
            Some(&mut data_size),
        );
        if size_result.is_err() || data_size == 0 {
            let _ = RegCloseKey(hkey);
            return None;
        }

        let mut buffer = vec![0u8; data_size as usize];
        let query_result = RegQueryValueExW(
            hkey,
            PCWSTR::from_raw(value_name_wide.as_ptr()),
            None,
            None,
            Some(buffer.as_mut_ptr()),
            Some(&mut data_size),
        );
        let _ = RegCloseKey(hkey);

        if query_result.is_err() {
            return None;
        }

        let wide: Vec<u16> = buffer[..data_size as usize]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let value = String::from_utf16_lossy(&wide);
        let trimmed = value.trim_end_matches('\0').trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }
}

/// Best-effort last day of this device's operational logs: today's and (if
/// present) yesterday's daily-rotated log file from `paths.log_dir` (see
/// `init_logging`), redacted (`virtue_core::api::redact_secrets`) and trimmed
/// to the API's attachment size cap, keeping the most recent bytes.
fn recent_logs(paths: &ClientPaths) -> Option<Vec<u8>> {
    let today = chrono::Local::now().date_naive();
    let mut combined = String::new();

    for date in [today, today - chrono::Duration::days(1)] {
        let file_name = format!(
            "{}.{}.log",
            virtue_core::logging::DEFAULT_FILE_LOG_POLICY.file_name_prefix,
            date.format("%Y-%m-%d")
        );
        if let Ok(contents) = std::fs::read_to_string(paths.log_dir.join(file_name)) {
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

#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_start_monitoring() -> *mut c_char {
    into_error_ptr(resident_monitor::start_monitoring())
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_stop_monitoring() -> *mut c_char {
    into_error_ptr(resident_monitor::stop_monitoring())
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_stop_monitoring_from_tray_exit() -> *mut c_char {
    into_error_ptr(resident_monitor::stop_monitoring_from_tray_exit())
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_stop_monitoring_for_os_session_end() -> *mut c_char {
    into_error_ptr(resident_monitor::stop_monitoring_for_os_session_end())
}

/// Returns `{"outcome": …, "message": …}` once the forced capture's batch has
/// landed (or the wait timed out) — not the moment the capture is queued. A
/// failure comes back as a plain error string, which the C# side turns into an
/// exception like every other `into_json_ptr` call.
#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_force_capture() -> *mut c_char {
    into_json_ptr(
        resident_monitor::force_capture_now().map(|outcome| ForceCapturePayload {
            outcome: outcome.code().to_string(),
            message: outcome.message().to_string(),
        }),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_get_monitor_status_json() -> *mut c_char {
    into_json_ptr(Ok(MonitorStatusPayload::from(
        resident_monitor::status_snapshot(),
    )))
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `value` must have been returned by this library via `CString::into_raw`
/// and must not be freed more than once.
pub unsafe extern "C" fn virtue_windows_free_string(value: *mut c_char) {
    if value.is_null() {
        return;
    }

    let _ = unsafe { CString::from_raw(value) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};
    use uuid::Uuid;

    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn test_lock() -> &'static Mutex<()> {
        TEST_LOCK.get_or_init(|| Mutex::new(()))
    }

    struct ProgramDataGuard {
        previous: Option<OsString>,
    }

    impl ProgramDataGuard {
        fn set(path: &std::path::Path) -> Self {
            let previous = std::env::var_os("PROGRAMDATA");
            unsafe {
                std::env::set_var("PROGRAMDATA", path);
            }
            Self { previous }
        }
    }

    impl Drop for ProgramDataGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => unsafe {
                    std::env::set_var("PROGRAMDATA", value);
                },
                None => unsafe {
                    std::env::remove_var("PROGRAMDATA");
                },
            }
        }
    }

    fn temporary_paths(name: &str) -> ClientPaths {
        let base =
            std::env::temp_dir().join(format!("virtue-windows-ffi-{name}-{}", Uuid::new_v4()));
        ClientPaths::from_base_dir(base)
    }

    fn ptr_to_string(ptr: *mut c_char) -> String {
        let value = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        unsafe {
            virtue_windows_free_string(ptr);
        }
        value
    }

    fn c_value(value: &str) -> CString {
        CString::new(value).expect("test strings should not contain null bytes")
    }

    #[test]
    fn init_logging_is_idempotent_across_repeated_calls() {
        // `virtue_windows_init` (and therefore `init_logging`) can legitimately be
        // called more than once per process — the WinUI app may re-init after a
        // runtime config change. Calling it twice must not panic, and the log
        // directory should exist afterward.
        let _guard = test_lock().lock().expect("test lock");
        let paths = temporary_paths("logging-idempotent");
        let _program_data = ProgramDataGuard::set(&paths.base_dir);
        paths.ensure_dirs().expect("ensure dirs");

        init_logging(&paths);
        init_logging(&paths);

        assert!(LOG_GUARD.get().is_some(), "logging should be initialized");
    }

    #[test]
    fn ffi_string_can_be_freed() {
        let _guard = test_lock().lock().expect("test lock");
        let paths = temporary_paths("free");
        let _program_data = ProgramDataGuard::set(&paths.base_dir);
        paths.ensure_dirs().expect("ensure dirs");

        let init_ptr = virtue_windows_init();
        assert!(init_ptr.is_null(), "init should succeed");

        let payload = virtue_windows_get_session_status_json();
        assert!(!payload.is_null(), "status payload should exist");
        unsafe {
            virtue_windows_free_string(payload);
        }
    }

    #[test]
    fn session_status_json_contract_is_stable() {
        let _guard = test_lock().lock().expect("test lock");
        let paths = temporary_paths("status");
        let _program_data = ProgramDataGuard::set(&paths.base_dir);
        paths.ensure_dirs().expect("ensure dirs");

        let payload = ptr_to_string(virtue_windows_get_session_status_json());
        let json: Value = serde_json::from_str(&payload).expect("valid json");

        assert_eq!(json["loggedIn"], false);
        assert!(json.get("buildLabel").is_some());
        assert!(json.get("email").is_some());
    }

    #[test]
    fn login_returns_a_useful_error_string_for_missing_credentials() {
        let _guard = test_lock().lock().expect("test lock");
        let request = c_value(r#"{"email":"","password":""}"#);
        let error = ptr_to_string(virtue_windows_login(request.as_ptr()));
        assert_eq!(error, "email is required");
    }

    #[test]
    fn monitor_status_json_contract_is_stable() {
        let _guard = test_lock().lock().expect("test lock");
        let payload = ptr_to_string(virtue_windows_get_monitor_status_json());
        let json: Value = serde_json::from_str(&payload).expect("valid json");

        assert!(json.get("state").is_some());
        assert!(json.get("loggedIn").is_some());
        assert!(json.get("pendingRequestCount").is_some());
        assert!(json.get("lastError").is_some());
        // The shared status-page fields (CORE-010) the WinUI dialog renders.
        for key in [
            "accountEmail",
            "deviceId",
            "deviceName",
            "partnerCount",
            "pendingHashCount",
            "pendingBatchCount",
            "lastLoopAtMs",
            "lastScreenshotAttemptAtMs",
            "lastScreenshotAtMs",
            "lastSkipReason",
            "lastBatchAtMs",
            "recentErrors",
            "apiBaseUrl",
            "hashBaseUrl",
            "captureIntervalSeconds",
            "batchWindowSeconds",
            "logDirectory",
        ] {
            assert!(json.get(key).is_some(), "missing key: {key}");
        }
    }

    #[test]
    fn force_capture_returns_a_useful_error_string_when_monitoring_is_not_running() {
        let _guard = test_lock().lock().expect("test lock");
        let error = ptr_to_string(virtue_windows_force_capture());
        assert_eq!(error, "monitoring is not running");
    }

    #[test]
    fn report_issue_returns_a_useful_error_string_for_empty_message() {
        let _guard = test_lock().lock().expect("test lock");
        let request = c_value(r#"{"message":"   "}"#);
        let error = ptr_to_string(virtue_windows_report_issue(request.as_ptr()));
        assert_eq!(error, "message is required");
    }

    #[test]
    fn windows_platform_details_is_never_empty() {
        assert!(!windows_platform_details().is_empty());
    }

    #[test]
    fn recent_logs_returns_none_when_no_log_files_exist() {
        let _guard = test_lock().lock().expect("test lock");
        let paths = temporary_paths("recent-logs-none");
        paths.ensure_dirs().expect("ensure dirs");

        assert!(recent_logs(&paths).is_none());
    }

    #[test]
    fn recent_logs_redacts_secrets_and_returns_todays_log() {
        let _guard = test_lock().lock().expect("test lock");
        let paths = temporary_paths("recent-logs-present");
        paths.ensure_dirs().expect("ensure dirs");

        let today = chrono::Local::now().date_naive().format("%Y-%m-%d");
        let file_name = format!("virtue.{today}.log");
        std::fs::write(
            paths.log_dir.join(file_name),
            "auth failed: token wst_AbCdEf123456ghijklmnop rejected\n",
        )
        .expect("write log file");

        let logs = recent_logs(&paths).expect("logs should be present");
        let text = String::from_utf8(logs).expect("logs should be utf8");
        assert!(!text.contains("wst_"));
        assert!(text.contains("[redacted]"));
    }
}
