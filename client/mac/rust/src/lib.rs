use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use once_cell::sync::OnceCell;
use virtue_core::{AuthState, ClientController};
use virtue_mac_platform::capture::{has_screen_capture_access, request_screen_capture_access};
use virtue_mac_platform::config::{ClientPaths, ClientState, read_auth_state, save_state};
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
/// `StatusRequest`.
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

/// Send `UserStopRequested` to the daemon and wait for a status round-trip on
/// the same connection, guaranteeing the daemon processed it before the
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
        Err(err) => string_to_c(err.to_string()),
    }
}
