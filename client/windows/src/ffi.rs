use std::ffi::{CStr, CString, c_char};
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::config::{
    ClientPaths, ResolvedRuntimeConfig, RuntimeConfigOverrides, default_device_name,
    load_runtime_overrides, resolved_runtime_config, save_runtime_overrides,
};
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
/// daily-rotated plain-text logs to `<data>\logs\virtue.log`. Subsequent
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

        let file_appender = tracing_appender::rolling::daily(
            &paths.log_dir,
            virtue_core::logging::DEFAULT_FILE_LOG_POLICY.file_name_prefix,
        );
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(virtue_core::logging::default_filter_directive(
                cfg!(debug_assertions),
            ))
        });
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(non_blocking)
            .with_ansi(false)
            .init();

        guard
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
struct RuntimeConfigPayload {
    api_base_url: String,
    capture_interval_seconds: u64,
    batch_window_seconds: u64,
    config_path: String,
    build_label: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MonitorStatusPayload {
    state: String,
    logged_in: bool,
    pending_request_count: usize,
    last_error: Option<String>,
}

impl From<MonitorStatusSnapshot> for MonitorStatusPayload {
    fn from(value: MonitorStatusSnapshot) -> Self {
        Self {
            state: value.state,
            logged_in: value.logged_in,
            pending_request_count: value.pending_request_count,
            last_error: value.last_error,
        }
    }
}

impl RuntimeConfigPayload {
    fn from_resolved_config(value: ResolvedRuntimeConfig, config_path: PathBuf) -> Self {
        Self {
            api_base_url: value.api_base_url,
            capture_interval_seconds: value.capture_interval_seconds,
            batch_window_seconds: value.batch_window_seconds,
            config_path: config_path.display().to_string(),
            build_label: virtue_core::BUILD_LABEL.to_string(),
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

#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_init(
    base_api_url: *const c_char,
    capture_interval_seconds: *const c_char,
    batch_window_seconds: *const c_char,
) -> *mut c_char {
    into_error_ptr((|| {
        let paths = ensure_paths_initialized()?;
        init_logging(&paths);

        let base_api_url = c_string_or_empty(base_api_url);
        let capture_interval_seconds = c_string_or_empty(capture_interval_seconds);
        let batch_window_seconds = c_string_or_empty(batch_window_seconds);

        if base_api_url.is_empty()
            && capture_interval_seconds.is_empty()
            && batch_window_seconds.is_empty()
        {
            return Ok(());
        }

        let mut overrides = load_runtime_overrides(&paths.runtime_config_file)?;
        if !base_api_url.is_empty() {
            overrides.api_base_url = Some(base_api_url);
        }
        if !capture_interval_seconds.is_empty() {
            overrides.capture_interval_seconds = Some(
                capture_interval_seconds
                    .parse()
                    .context("capture_interval_seconds must be an integer")?,
            );
        }
        if !batch_window_seconds.is_empty() {
            overrides.batch_window_seconds = Some(
                batch_window_seconds
                    .parse()
                    .context("batch_window_seconds must be an integer")?,
            );
        }

        save_runtime_overrides(&paths.runtime_config_file, &overrides)
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

#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_logout() -> *mut c_char {
    into_error_ptr((|| {
        let manager = with_session_manager()?;
        manager.logout_blocking()?;
        Ok(())
    })())
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_get_runtime_config_json() -> *mut c_char {
    into_json_ptr((|| {
        let paths = current_paths()?;
        paths.ensure_dirs()?;
        let resolved = resolved_runtime_config(&paths)?;
        Ok(RuntimeConfigPayload::from_resolved_config(
            resolved,
            paths.runtime_config_file,
        ))
    })())
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

#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_get_monitor_status_json() -> *mut c_char {
    into_json_ptr(Ok(MonitorStatusPayload::from(
        resident_monitor::status_snapshot(),
    )))
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_windows_set_runtime_config_json(
    config_json: *const c_char,
) -> *mut c_char {
    into_error_ptr((|| {
        let paths = current_paths()?;
        paths.ensure_dirs()?;

        let config_json = c_string_or_empty(config_json);
        let overrides: RuntimeConfigOverrides =
            serde_json::from_str(&config_json).context("failed parsing runtime config json")?;
        save_runtime_overrides(&paths.runtime_config_file, &overrides)?;
        Ok(())
    })())
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

        let init_ptr = virtue_windows_init(std::ptr::null(), std::ptr::null(), std::ptr::null());
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
    fn runtime_config_round_trip_works_through_ffi() {
        let _guard = test_lock().lock().expect("test lock");
        let paths = temporary_paths("config");
        let _program_data = ProgramDataGuard::set(&paths.base_dir);
        let base_url = c_value("https://dev-api.example.com");
        let capture = c_value("45");
        let batch = c_value("90");

        let init_ptr = virtue_windows_init(base_url.as_ptr(), capture.as_ptr(), batch.as_ptr());
        assert!(init_ptr.is_null(), "init should succeed");

        let payload = ptr_to_string(virtue_windows_get_runtime_config_json());
        let json: Value = serde_json::from_str(&payload).expect("valid json");

        assert_eq!(json["apiBaseUrl"], "https://dev-api.example.com");
        assert_eq!(json["captureIntervalSeconds"], 45);
        assert_eq!(json["batchWindowSeconds"], 90);

        let replacement = c_value(
            r#"{"apiBaseUrl":"https://api2.example.com","captureIntervalSeconds":60,"batchWindowSeconds":120}"#,
        );
        let update_ptr = virtue_windows_set_runtime_config_json(replacement.as_ptr());
        assert!(update_ptr.is_null(), "runtime config update should succeed");

        let updated_payload = ptr_to_string(virtue_windows_get_runtime_config_json());
        let updated: Value = serde_json::from_str(&updated_payload).expect("valid json");
        assert_eq!(updated["apiBaseUrl"], "https://api2.example.com");
        assert_eq!(updated["captureIntervalSeconds"], 60);
        assert_eq!(updated["batchWindowSeconds"], 120);
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
    }
}
