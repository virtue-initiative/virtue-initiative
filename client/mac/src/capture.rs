use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use uuid::Uuid;
use virtue_core::{
    CoreError, CoreResult, LifecycleHooks, PlatformHooks, Screenshot, ScreenshotHooks,
};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

pub fn capture_screen() -> Result<Vec<u8>> {
    if !has_screen_capture_access() {
        return Err(anyhow!(
            "screen recording permission missing (grant Screen Recording permission in macOS)"
        ));
    }

    run_capture_command("/usr/sbin/screencapture", &["-x", "-t", "png"])
        .or_else(|_| run_capture_command("screencapture", &["-x", "-t", "png"]))
        .with_context(|| "screencapture failed (grant Screen Recording permission in macOS)")
}

pub fn has_screen_capture_access() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Request screen capture access and return whether it is now granted.
pub fn request_screen_capture_access() -> bool {
    if has_screen_capture_access() {
        return true;
    }

    let _ = unsafe { CGRequestScreenCaptureAccess() };

    // Some macOS/TCC states do not visibly present the prompt from
    // CGRequestScreenCaptureAccess alone. Make one explicit throwaway capture
    // attempt for user-initiated permission flows, but never return these bytes
    // to core so a denied black capture cannot be uploaded.
    let _ = run_capture_command("/usr/sbin/screencapture", &["-x", "-t", "png"])
        .or_else(|_| run_capture_command("screencapture", &["-x", "-t", "png"]));

    has_screen_capture_access()
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

// Parses a single `last -F` line's trailing date (e.g.
// "shutdown  ~          Mon Jun 10 22:45:00 2024") into milliseconds since
// epoch. The `-F` flag on macOS BSD `last` includes the year and seconds.
// Returns None if the date is unparseable.
fn parse_last_line_date(line: &str) -> Option<i64> {
    use chrono::{Local, NaiveDateTime, TimeZone};

    let tokens: Vec<&str> = line.split_whitespace().collect();
    for w in tokens.windows(5) {
        let Ok(day) = w[2].parse::<u32>() else {
            continue;
        };
        let normalized = format!("{} {} {:02} {} {}", w[0], w[1], day, w[3], w[4]);
        if let Ok(dt) = NaiveDateTime::parse_from_str(&normalized, "%a %b %d %H:%M:%S %Y")
            && let chrono::LocalResult::Single(local) = Local.from_local_datetime(&dt)
        {
            return Some(local.timestamp_millis());
        }
    }
    None
}

// Parses `last -1 -F shutdown` output and returns the shutdown time in milliseconds.
// Returns None if no shutdown line is found or the date is unparseable. This is a
// floor/approximation of the true logout/shutdown time, not exact — see
// `LifecycleHooks::get_last_logout_utc_ms`.
fn parse_last_shutdown_mac(s: &str) -> Option<i64> {
    s.lines()
        .find(|line| line.starts_with("shutdown"))
        .and_then(parse_last_line_date)
}

// Parses unfiltered `last -F` output and returns the most recent real login's
// timestamp in milliseconds, skipping the "reboot"/"shutdown" pseudo-user
// entries `last` also logs.
fn parse_last_login_mac(s: &str) -> Option<i64> {
    s.lines()
        .map(str::trim_start)
        .find(|line| {
            !line.is_empty()
                && !line.starts_with("reboot")
                && !line.starts_with("shutdown")
                && !line.starts_with("wtmp begins")
        })
        .and_then(parse_last_line_date)
}

#[derive(Clone, Default)]
pub struct MacPlatformHooks;

impl MacPlatformHooks {
    pub fn new() -> Self {
        Self
    }
}

impl ScreenshotHooks for MacPlatformHooks {
    fn take_screenshot(&self) -> CoreResult<Screenshot> {
        let bytes = capture_screen().map_err(|err| CoreError::CommandFailed(err.to_string()))?;
        Ok(Screenshot {
            captured_at_ms: self.get_time_utc_ms()?,
            bytes,
            content_type: "image/png".to_string(),
        })
    }

    fn is_locked_or_screensaver(&self) -> CoreResult<bool> {
        // Either the session being locked or an active screensaver means the user
        // can't be viewing real content. Both checks fail safe to false.
        Ok(screen_is_locked() || screensaver_process_running())
    }
}

// Reads `CGSessionCopyCurrentDictionary()["CGSSessionScreenIsLocked"]`. Returns false
// when the dictionary or key is absent (unknown session) — the fail-safe default.
fn screen_is_locked() -> bool {
    use std::ffi::c_void;

    use objc2_core_foundation::{CFBoolean, CFString};
    use objc2_core_graphics::CGSessionCopyCurrentDictionary;

    let Some(dict) = CGSessionCopyCurrentDictionary() else {
        return false;
    };
    let key = CFString::from_str("CGSSessionScreenIsLocked");
    // SAFETY: `dict` is a valid CFDictionary; `key` outlives the lookup. The returned
    // pointer (if non-null) is the dictionary's CFBoolean for that key.
    let value = unsafe { dict.value(&*key as *const CFString as *const c_void) };
    if value.is_null() {
        return false;
    }
    // The screensaver/lock value is the shared kCFBooleanTrue singleton; pointer
    // identity against it is the simplest correct read.
    let true_ptr = CFBoolean::new(true) as *const CFBoolean as *const c_void;
    std::ptr::eq(value, true_ptr)
}

// Detects the macOS screensaver host process. The engine name differs across releases.
fn screensaver_process_running() -> bool {
    ["ScreenSaverEngine", "legacyScreenSaver"]
        .iter()
        .any(|name| {
            Command::new("pgrep")
                .args(["-x", name])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        })
}

#[repr(C)]
struct MachTimebaseInfo {
    numer: u32,
    denom: u32,
}

unsafe extern "C" {
    fn mach_continuous_time() -> u64;
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
}

fn mach_ticks_to_ms(ticks: u64) -> CoreResult<i64> {
    let mut info = MachTimebaseInfo { numer: 0, denom: 0 };
    // SAFETY: `info` is a valid, owned out-param; `mach_timebase_info` does not
    // retain the pointer past the call.
    let rc = unsafe { mach_timebase_info(&mut info) };
    if rc != 0 {
        return Err(CoreError::CommandFailed(format!(
            "mach_timebase_info failed: {rc}"
        )));
    }
    let ns = u128::from(ticks) * u128::from(info.numer) / u128::from(info.denom);
    Ok((ns / 1_000_000) as i64)
}

impl LifecycleHooks for MacPlatformHooks {
    fn get_boot_clock_ms(&self) -> CoreResult<i64> {
        // SAFETY: `mach_continuous_time` takes no arguments and has no
        // preconditions.
        mach_ticks_to_ms(unsafe { mach_continuous_time() })
    }

    fn get_monotonic_clock_ms(&self) -> CoreResult<i64> {
        // SAFETY: `mach_absolute_time` takes no arguments and has no
        // preconditions.
        mach_ticks_to_ms(unsafe { mach_absolute_time() })
    }

    fn get_last_login_utc_ms(&self) -> CoreResult<Option<i64>> {
        let output = Command::new("last")
            .args(["-5", "-F"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok();
        let Some(output) = output else {
            return Ok(None);
        };
        if !output.status.success() {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(parse_last_login_mac(&text))
    }

    fn get_last_logout_utc_ms(&self) -> CoreResult<Option<i64>> {
        let output = Command::new("last")
            .args(["-1", "-F", "shutdown"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok();
        let Some(output) = output else {
            return Ok(None);
        };
        if !output.status.success() {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(parse_last_shutdown_mac(&text))
    }
}

impl PlatformHooks for MacPlatformHooks {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_last_shutdown_mac_extracts_timestamp_from_last_output() {
        // Standard `last -F shutdown` output format
        let input = "shutdown  ~          Mon Jun 10 22:45:00 2024\n\nwtmp begins ...\n";
        let result = parse_last_shutdown_mac(input);
        assert!(result.is_some(), "expected Some timestamp, got None");
        // Should be somewhere around 2024-06-10 (1717286400000 ± timezone offset)
        let ms = result.unwrap();
        assert!(ms > 1_700_000_000_000, "timestamp not in expected range");
        assert!(ms < 1_800_000_000_000, "timestamp not in expected range");
    }

    #[test]
    fn parse_last_shutdown_mac_handles_single_digit_day() {
        // Space-padded day in original output becomes bare digit after split_whitespace.
        // Jun 5 2024 was a Wednesday; the weekday must match the date or chrono rejects it.
        let input = "shutdown  ~          Wed Jun  5 09:00:00 2024\n";
        let result = parse_last_shutdown_mac(input);
        assert!(
            result.is_some(),
            "expected Some timestamp for single-digit day"
        );
    }

    #[test]
    fn parse_last_shutdown_mac_returns_none_when_no_shutdown_line() {
        let input = "reboot    ~          Mon Jun 10 22:45:00 2024\n\nwtmp begins ...\n";
        assert_eq!(parse_last_shutdown_mac(input), None);
    }

    #[test]
    fn parse_last_shutdown_mac_returns_none_on_empty_input() {
        assert_eq!(parse_last_shutdown_mac(""), None);
    }

    #[test]
    fn parse_last_login_mac_extracts_most_recent_real_login() {
        let input = "alice     console              Mon Jun 10 22:45:00 2024 - 23:00:00 (00:15)\n\nwtmp begins Mon Jun  3 08:00:00 2024\n";
        let result = parse_last_login_mac(input);
        assert!(result.is_some(), "expected Some timestamp, got None");
        let ms = result.unwrap();
        assert!(ms > 1_700_000_000_000, "timestamp not in expected range");
        assert!(ms < 1_800_000_000_000, "timestamp not in expected range");
    }

    #[test]
    fn parse_last_login_mac_skips_reboot_and_shutdown_pseudo_entries() {
        let input = "reboot    ~                   Mon Jun 10 22:00:00 2024\nalice     console              Mon Jun 10 21:45:00 2024 - 22:00:00 (00:15)\n";
        let result = parse_last_login_mac(input);
        assert!(result.is_some(), "expected Some timestamp, got None");
    }

    #[test]
    fn parse_last_login_mac_returns_none_when_only_pseudo_entries() {
        let input = "reboot    ~                   Mon Jun 10 22:00:00 2024\nshutdown  ~                   Mon Jun 10 21:00:00 2024\n\nwtmp begins Mon Jun  3 08:00:00 2024\n";
        assert_eq!(parse_last_login_mac(input), None);
    }
}
