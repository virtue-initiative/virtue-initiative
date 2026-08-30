use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};
use once_cell::sync::OnceCell;
use virtue_core::AuthState;
use virtue_core::force_capture;
use virtue_core::ipc::ClientController;
use virtue_mac_platform::capture::{has_screen_capture_access, request_screen_capture_access};
use virtue_mac_platform::config::{
    ClientPaths, ClientState, build_core_config, read_auth_state, save_state,
};
use virtue_mac_platform::launch_agent;

static CORE: OnceCell<MacCore> = OnceCell::new();

const DAEMON_STATUS_RUNNING: c_int = 0;
const DAEMON_STATUS_STOPPED: c_int = 1;
const DAEMON_STATUS_UNREACHABLE: c_int = 2;

struct MacCore {
    paths: ClientPaths,
}

/// Initialize the FFI layer, resolving the same OS-standard directories the
/// daemon uses (`~/.config/virtue`, `~/Library/Application Support/virtue`,
/// `~/Library/LaunchAgents`, `~/Library/Logs`). Unlike iOS, macOS is
/// unsandboxed, so these paths don't need to be supplied by the caller.
#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_init() -> *mut c_char {
    let result = (|| -> Result<()> {
        let paths = ClientPaths::discover()?;
        paths.ensure_dirs()?;
        if CORE.get().is_none() {
            CORE.set(MacCore { paths })
                .map_err(|_| anyhow!("core already initialized"))?;
        }
        Ok(())
    })();
    into_c_result(result)
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_login(
    email: *const c_char,
    password: *const c_char,
    device_name: *const c_char,
) -> *mut c_char {
    let result = (|| -> Result<()> {
        let core = core()?;
        let email = c_string_or_empty(email);
        let password = c_string_or_empty(password);
        let device_name = c_string_or_empty(device_name);
        let sock = core.paths.state_dir.join("daemon.sock");
        let mut client = ClientController::connect(&sock)
            .context("failed to connect to daemon (is it running?)")?;
        client
            .login(&email, &password, Some(&device_name))
            .context("login failed")?;
        save_state(
            &core.paths.ui_state_file,
            &ClientState { email: Some(email) },
        )?;
        Ok(())
    })();
    into_c_result(result)
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_logout() -> *mut c_char {
    let result = (|| -> Result<()> {
        let core = core()?;
        let sock = core.paths.state_dir.join("daemon.sock");
        let mut client = ClientController::connect(&sock)
            .context("failed to connect to daemon (is it running?)")?;
        client.logout().context("logout failed")?;
        // Do NOT stop the daemon — the app remains open and the user can log
        // back in.
        save_state(&core.paths.ui_state_file, &ClientState { email: None })?;
        Ok(())
    })();
    into_c_result(result)
}

/// Submits `POST /bug-report` (API-042). `contact_email` is treated as unset
/// when blank. Reads the device's refresh token straight off disk, same
/// disk-fallback approach Linux/Windows use, so a report can be attributed to
/// this device even when the resident daemon isn't reachable over IPC;
/// gathers macOS version info via `sw_vers`; and, when `include_logs` is
/// true, reads/redacts/trims the last two days of the daemon's own
/// rotated log files (`paths.logs_dir`) for the optional attachment.
#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_report_issue(
    message: *const c_char,
    contact_email: *const c_char,
    include_logs: bool,
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

        let bearer_token = local_auth_state(core)
            .device_credentials
            .map(|creds| creds.refresh_token);

        let platform_details = mac_platform_details();
        let logs = include_logs.then(|| recent_logs(&core.paths)).flatten();

        let config = build_core_config(&core.paths);
        let api = virtue_core::api::HttpApiClient::new(&config)?;
        api.report_issue(
            bearer_token.as_deref(),
            &virtue_core::api::BugReportRequest {
                message: &message,
                contact_email: contact_email.as_deref(),
                platform: "macos",
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

/// Best-effort "ProductName ProductVersion (Build BuildVersion)" string via
/// `sw_vers`, e.g. `"macOS 14.5 (Build 23F79)"`. Falls back to a fixed
/// placeholder on any error, mirroring the Windows client's registry-read
/// fallback in `windows_platform_details`.
fn mac_platform_details() -> String {
    let product_name = sw_vers("-productName");
    let product_version = sw_vers("-productVersion");
    let build_version = sw_vers("-buildVersion");

    let mut parts = Vec::new();
    if let Some(product_name) = product_name {
        parts.push(product_name);
    }

    let version_part = match (product_version, build_version) {
        (Some(version), Some(build)) => Some(format!("{version} (Build {build})")),
        (Some(version), None) => Some(version),
        (None, Some(build)) => Some(format!("Build {build}")),
        (None, None) => None,
    };
    if let Some(version_part) = version_part {
        parts.push(version_part);
    }

    if parts.is_empty() {
        "macOS (unknown version)".to_string()
    } else {
        parts.join(" ")
    }
}

fn sw_vers(flag: &str) -> Option<String> {
    Command::new("sw_vers")
        .arg(flag)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Best-effort last two days of this device's daemon logs: today's and (if
/// present) yesterday's daily-rotated log file from `paths.logs_dir` (see
/// `daemon::init_logging`), redacted (`virtue_core::api::redact_secrets`) and
/// trimmed to the API's attachment size cap, keeping the most recent bytes.
fn recent_logs(paths: &ClientPaths) -> Option<Vec<u8>> {
    let today = chrono::Local::now().date_naive();
    let mut combined = String::new();

    for date in [today, today - chrono::Duration::days(1)] {
        let file_name = format!(
            "{}.{}.log",
            virtue_core::logging::DEFAULT_FILE_LOG_POLICY.file_name_prefix,
            date.format("%Y-%m-%d")
        );
        if let Ok(contents) = std::fs::read_to_string(paths.logs_dir.join(file_name)) {
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
pub extern "C" fn virtue_mac_native_is_logged_in() -> bool {
    core()
        .map(|c| local_auth_state(c).device_credentials.is_some())
        .unwrap_or(false)
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_get_device_id() -> *mut c_char {
    let device_id = core()
        .ok()
        .and_then(|c| local_auth_state(c).device_credentials)
        .map(|d| d.device_id);
    optional_string_to_c(device_id)
}

/// The email address of the currently signed-in account, persisted to
/// `ui_state_file` at login (see `virtue_mac_native_login`), or null when
/// signed out. Used to pre-fill the "Report a Bug" contact-email field.
#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_get_account_email() -> *mut c_char {
    let email = core()
        .ok()
        .and_then(|c| virtue_mac_platform::config::load_state(&c.paths.ui_state_file).ok())
        .and_then(|state| state.email);
    optional_string_to_c(email)
}

/// Returns a JSON-serialized `ServiceStatus` (caller frees with
/// `virtue_mac_native_free_string`), or null if the daemon can't be reached.
#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_get_status_json() -> *mut c_char {
    let json = (|| -> Result<String> {
        let core = core()?;
        let sock = core.paths.state_dir.join("daemon.sock");
        let mut client = ClientController::connect(&sock)?;
        let status = client.get_status()?;
        Ok(serde_json::to_string(&status)?)
    })();
    match json {
        Ok(value) => string_to_c(value),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Poll the daemon, distinguishing a refused connection (daemon genuinely
/// stopped, `1`) from a timeout/IPC error (daemon alive but busy, `2`). A
/// successful status response always means running (`0`), since the
/// lifecycle module hardcodes `is_running: true` whenever it can answer a
/// status request at all.
#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_poll_daemon_status() -> c_int {
    let Ok(core) = core() else {
        return DAEMON_STATUS_STOPPED;
    };
    let sock = core.paths.state_dir.join("daemon.sock");
    match ClientController::connect(&sock) {
        Err(_) => DAEMON_STATUS_STOPPED,
        Ok(mut client) => match client.get_status() {
            Ok(_) => DAEMON_STATUS_RUNNING,
            Err(_) => DAEMON_STATUS_UNREACHABLE,
        },
    }
}

/// Tell the daemon a user requested a stop, and wait for a status round-trip
/// on the same connection, guaranteeing the daemon processed it before the
/// caller proceeds to stop the launch agent.
#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_request_user_stop(source: *const c_char) -> *mut c_char {
    let result = (|| -> Result<()> {
        let core = core()?;
        let source = c_string_or_empty(source);
        let sock = core.paths.state_dir.join("daemon.sock");
        let mut client = ClientController::connect(&sock)
            .context("failed to connect to daemon (is it running?)")?;
        client.request_user_stop(&source)?;
        let _ = client.get_status();
        Ok(())
    })();
    into_c_result(result)
}

/// Forces an immediate screenshot capture (bypassing the normal interval-due
/// gate, but still honoring the locked/screensaver and fingerprint-dedup
/// gates) and requests an immediate batch flush, so the result uploads
/// without waiting out the normal batch interval.
///
/// Then waits for the batch to actually land, so the UI can confirm what
/// really happened rather than guessing. Returns JSON: either
/// `{"outcome": …, "message": …}` (see `force_capture::ForcedCaptureOutcome`)
/// or `{"error": …}`.
#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_force_capture() -> *mut c_char {
    let result = (|| -> Result<String> {
        let core = core()?;
        let sock = core.paths.state_dir.join("daemon.sock");
        let before = {
            let mut client = ClientController::connect(&sock)
                .context("failed to connect to daemon (is it running?)")?;
            let before = client.get_status().context("failed to read status")?;
            client.force_capture_now().context("force capture failed")?;
            before
        };
        // The daemon serves one connection at a time, so poll on a fresh
        // connection each time rather than holding the socket — and the app's
        // own status polling — for the whole wait.
        let outcome = force_capture::wait_for_upload(
            &before,
            force_capture::DEFAULT_UPLOAD_TIMEOUT,
            force_capture::DEFAULT_POLL_INTERVAL,
            || ClientController::connect(&sock)?.get_status(),
            std::thread::sleep,
        )
        .context("failed to read status while waiting for the upload")?;
        Ok(outcome.report_json())
    })();
    match result {
        Ok(json) => string_to_c(json),
        Err(err) => string_to_c(serde_json::json!({ "error": format!("{err:#}") }).to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_has_capture_permission() -> bool {
    has_screen_capture_access()
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_request_capture_permission() -> bool {
    request_screen_capture_access()
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_ensure_daemon_running(
    daemon_exe_path: *const c_char,
) -> *mut c_char {
    let result = (|| -> Result<()> {
        let core = core()?;
        let exe = c_string_or_empty(daemon_exe_path);
        launch_agent::ensure_agent_running(&core.paths, Path::new(&exe))
    })();
    into_c_result(result)
}

/// Stop the background daemon. When `user_initiated` is true, first tell the
/// daemon a user requested the stop so it records a clean user stop (which
/// fires a stop-time alert) instead of being classified as an unexpected
/// `Other` stop that would trigger an unexpected-start alert on next launch.
#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_stop_daemon(user_initiated: bool) -> *mut c_char {
    let result = (|| -> Result<()> {
        let core = core()?;
        if user_initiated {
            let sock = core.paths.state_dir.join("daemon.sock");
            if let Ok(mut client) = ClientController::connect(&sock)
                && client.request_user_stop("mac_app_stop").is_ok()
            {
                let _ = client.get_status();
            }
        }
        launch_agent::stop_agent(&core.paths)
    })();
    into_c_result(result)
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_relaunch_daemon(daemon_exe_path: *const c_char) -> *mut c_char {
    let result = (|| -> Result<()> {
        let core = core()?;
        let exe = c_string_or_empty(daemon_exe_path);
        if agent_is_registered(core) {
            launch_agent::stop_agent(&core.paths)
                .context("failed to stop existing background service before relaunch")?;
        }
        launch_agent::ensure_agent_running(&core.paths, Path::new(&exe))
            .context("failed to relaunch background service")?;
        Ok(())
    })();
    into_c_result(result)
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_agent_is_registered() -> bool {
    core().map(agent_is_registered).unwrap_or(false)
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_get_build_label() -> *mut c_char {
    string_to_c(virtue_core::BUILD_LABEL.to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_default_device_name() -> *mut c_char {
    string_to_c(virtue_mac_platform::config::default_device_name())
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_default_capture_interval_seconds() -> u64 {
    virtue_core::default_capture_interval_seconds()
}

#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_default_batch_window_seconds() -> u64 {
    virtue_core::default_batch_window_seconds()
}

/// Resolve the daemon executable bundled inside the app at
/// `Contents/MacOS/virtue-daemon`, given the app bundle path
/// (`Bundle.main.bundlePath` on the Swift side).
#[unsafe(no_mangle)]
pub extern "C" fn virtue_mac_native_daemon_exe_path(app_bundle_path: *const c_char) -> *mut c_char {
    let bundle = c_string_or_empty(app_bundle_path);
    let path: PathBuf = Path::new(&bundle)
        .join("Contents")
        .join("MacOS")
        .join("virtue-daemon");
    string_to_c(path.display().to_string())
}

/// # Safety
///
/// `value` must have been returned by this library via `CString::into_raw`
/// and must not be freed more than once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn virtue_mac_native_free_string(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    let _ = unsafe { CString::from_raw(value) };
}

fn agent_is_registered(core: &MacCore) -> bool {
    core.paths.launch_agent_file.exists() || launch_agent::is_agent_loaded().unwrap_or(false)
}

fn local_auth_state(core: &MacCore) -> AuthState {
    read_auth_state(&core.paths.state_dir).unwrap_or_default()
}

fn core() -> Result<&'static MacCore> {
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

fn string_to_c(value: String) -> *mut c_char {
    CString::new(value)
        .map(CString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}

fn optional_string_to_c(value: Option<String>) -> *mut c_char {
    match value {
        Some(value) => string_to_c(value),
        None => std::ptr::null_mut(),
    }
}

fn into_c_result(result: Result<()>) -> *mut c_char {
    match result {
        Ok(()) => std::ptr::null_mut(),
        Err(err) => string_to_c(format!("{err:#}")),
    }
}
