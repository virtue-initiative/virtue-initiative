use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use uuid::Uuid;
use virtue_core::{CoreError, CoreResult, PlatformHooks, Screenshot};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
}

pub fn capture_screen() -> Result<Vec<u8>> {
    run_capture_command("/usr/sbin/screencapture", &["-x", "-t", "png"])
        .or_else(|_| run_capture_command("screencapture", &["-x", "-t", "png"]))
        .with_context(|| "screencapture failed (grant Screen Recording permission in macOS)")
}

pub fn has_screen_capture_access() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

pub fn is_permission_missing_error(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    normalized.contains("screen recording")
        || normalized.contains("not permitted")
        || normalized.contains("permission")
}

fn run_capture_command(cmd: &str, args: &[&str]) -> Result<Vec<u8>> {
    let output_path = temporary_capture_path();
    let output_path_str = output_path.display().to_string();

    let output = Command::new(cmd)
        .args(args)
        .arg(&output_path_str)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .output()
        .with_context(|| format!("failed to execute {cmd}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = fs::remove_file(&output_path);
        return Err(anyhow!("{} exited with {}: {}", cmd, output.status, stderr));
    }

    let bytes =
        fs::read(&output_path).with_context(|| format!("failed reading {}", output_path_str))?;
    let _ = fs::remove_file(&output_path);

    if bytes.is_empty() {
        return Err(anyhow!("{} returned empty output file", cmd));
    }

    Ok(bytes)
}

fn temporary_capture_path() -> PathBuf {
    let file_name = format!("virtue-capture-{}.png", Uuid::new_v4());
    std::env::temp_dir().join(file_name)
}

#[derive(Clone)]
pub struct MacPlatformHooks;

impl MacPlatformHooks {
    pub fn new() -> Self {
        Self
    }
}

impl PlatformHooks for MacPlatformHooks {
    fn take_screenshot(&self) -> CoreResult<Screenshot> {
        let bytes = capture_screen().map_err(|err| CoreError::CommandFailed(err.to_string()))?;
        Ok(Screenshot {
            captured_at_ms: self.get_time_utc_ms()?,
            bytes,
            content_type: "image/png".to_string(),
        })
    }

    fn get_time_utc_ms(&self) -> CoreResult<i64> {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| CoreError::CommandFailed(err.to_string()))?;
        i64::try_from(duration.as_millis())
            .map_err(|_| CoreError::InvalidState("system clock overflow"))
    }
}
