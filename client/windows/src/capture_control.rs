#![cfg(target_os = "windows")]

use std::ffi::{OsStr, c_void};
use std::fs;
use std::io::ErrorKind;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Result, anyhow, bail};
use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_NO_TOKEN, HANDLE};
use windows::Win32::Security::{
    DuplicateTokenEx, SecurityImpersonation, TOKEN_ACCESS_MASK, TOKEN_ADJUST_DEFAULT,
    TOKEN_ADJUST_SESSIONID, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY, TokenPrimary,
};
use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows::Win32::System::RemoteDesktop::{WTSGetActiveConsoleSessionId, WTSQueryUserToken};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, MUTEX_MODIFY_STATE,
    OpenMutexW, PROCESS_INFORMATION, STARTUPINFOW, SYNCHRONIZATION_SYNCHRONIZE,
};
use windows::core::{PCWSTR, PWSTR, w};

use crate::config::ClientPaths;

pub const CAPTURE_INSTANCE_MUTEX_NAME: PCWSTR = w!("Global\\VirtueCaptureConsole");
const CAPTURE_STOP_SIGNAL_FILE: &str = "capture.stop";

pub fn is_capture_running() -> bool {
    unsafe {
        let handle = OpenMutexW(
            MUTEX_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE,
            false,
            CAPTURE_INSTANCE_MUTEX_NAME,
        );
        match handle {
            Ok(handle) => {
                let _ = CloseHandle(handle);
                true
            }
            Err(err) => err.code() != ERROR_FILE_NOT_FOUND.into(),
        }
    }
}

pub fn clear_capture_stop_signal(paths: &ClientPaths) -> Result<()> {
    match fs::remove_file(capture_stop_signal_path(paths)) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    Ok(())
}

pub fn signal_capture_stop(paths: &ClientPaths) -> Result<()> {
    let signal_path = capture_stop_signal_path(paths);
    if let Some(parent) = signal_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(signal_path, b"stop")?;
    Ok(())
}

pub fn is_capture_stop_requested(paths: &ClientPaths) -> bool {
    capture_stop_signal_path(paths).exists()
}

pub fn launch_capture_in_active_session(paths: &ClientPaths) -> Result<Option<u32>> {
    let capture_exe = resolve_capture_executable(paths);
    if !capture_exe.exists() {
        bail!("capture executable missing: {}", capture_exe.display());
    }

    let session_id = unsafe { WTSGetActiveConsoleSessionId() };
    if session_id == u32::MAX {
        return Ok(None);
    }

    let mut user_token = HANDLE::default();
    if let Err(err) = unsafe { WTSQueryUserToken(session_id, &mut user_token) } {
        if err.code() == ERROR_FILE_NOT_FOUND.into() || err.code() == ERROR_NO_TOKEN.into() {
            return Ok(None);
        }
        return Err(anyhow!("WTSQueryUserToken failed: {err}"));
    }

    let mut primary_token = HANDLE::default();
    let desired_access: TOKEN_ACCESS_MASK = TOKEN_ASSIGN_PRIMARY
        | TOKEN_DUPLICATE
        | TOKEN_QUERY
        | TOKEN_ADJUST_DEFAULT
        | TOKEN_ADJUST_SESSIONID;

    let result = (|| -> Result<Option<u32>> {
        unsafe {
            DuplicateTokenEx(
                user_token,
                desired_access,
                None,
                SecurityImpersonation,
                TokenPrimary,
                &mut primary_token,
            )?;
        }

        let mut environment: *mut c_void = std::ptr::null_mut();
        unsafe {
            CreateEnvironmentBlock(&mut environment, Some(primary_token), false)?;
        }

        let app = to_wide_null(capture_exe.as_os_str());
        let current_dir_path = capture_exe
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| paths.base_dir.clone());
        let current_dir = to_wide_null(current_dir_path.as_os_str());
        let mut cmd = to_wide_null(format!(
            "\"{}\" --mode capture --console",
            capture_exe.display()
        ));
        let mut desktop = to_wide_null("winsta0\\default");

        let startup_info = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            lpDesktop: PWSTR(desktop.as_mut_ptr()),
            ..Default::default()
        };

        let mut proc_info = PROCESS_INFORMATION::default();
        let create_result = unsafe {
            CreateProcessAsUserW(
                Some(primary_token),
                PCWSTR(app.as_ptr()),
                Some(PWSTR(cmd.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
                Some(environment),
                PCWSTR(current_dir.as_ptr()),
                &startup_info,
                &mut proc_info,
            )
        };

        let destroy_env_result = unsafe { DestroyEnvironmentBlock(environment as _) };
        if let Err(err) = destroy_env_result {
            return Err(anyhow!("DestroyEnvironmentBlock failed: {err}"));
        }

        create_result?;
        unsafe {
            let _ = CloseHandle(proc_info.hThread);
            let _ = CloseHandle(proc_info.hProcess);
        }
        Ok(Some(proc_info.dwProcessId))
    })();

    unsafe {
        let _ = CloseHandle(primary_token);
        let _ = CloseHandle(user_token);
    }

    result
}

fn capture_stop_signal_path(paths: &ClientPaths) -> PathBuf {
    paths.data_dir.join(CAPTURE_STOP_SIGNAL_FILE)
}

fn to_wide_null(value: impl AsRef<OsStr>) -> Vec<u16> {
    value
        .as_ref()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn resolve_capture_executable(paths: &ClientPaths) -> PathBuf {
    let mut candidates = Vec::new();
    if let Some(install_dir) = read_install_dir_from_registry() {
        candidates.push(install_dir.join("virtue-service.exe"));
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        candidates.push(
            PathBuf::from(program_files)
                .join("Virtue")
                .join("virtue-service.exe"),
        );
    }
    if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
        candidates.push(
            PathBuf::from(program_files_x86)
                .join("Virtue")
                .join("virtue-service.exe"),
        );
    }
    candidates.push(paths.base_dir.join("virtue-service.exe"));

    candidates
        .into_iter()
        .find(|candidate| candidate.exists())
        .unwrap_or_else(|| paths.base_dir.join("virtue-service.exe"))
}

fn read_install_dir_from_registry() -> Option<PathBuf> {
    let output = Command::new("reg")
        .args(["query", r"HKLM\Software\Virtue", "/v", "InstallDir"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if !line.contains("InstallDir") || !line.contains("REG_SZ") {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, "REG_SZ").collect();
        if let Some(path) = parts.get(1).map(|p| p.trim())
            && !path.is_empty()
        {
            return Some(PathBuf::from(path));
        }
    }
    None
}
