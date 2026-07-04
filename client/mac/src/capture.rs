use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use uuid::Uuid;
use virtue_core::{CoreError, CoreResult, PlatformHooks, Screenshot, ScreenshotHooks};

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

// Parses `sysctl -n kern.boottime` output (e.g. "{ sec = 1718000000, usec = 123456 } ...")
// and returns the boot time in milliseconds since epoch.
fn parse_boottime_ms(s: &str) -> Option<i64> {
    let mut search_start = 0;
    let sec_start = loop {
        let candidate = s[search_start..].find("sec = ")? + search_start;
        // Skip matches inside "usec = ", which contains "sec = " starting at its 2nd byte.
        if candidate > 0 && s.as_bytes()[candidate - 1] == b'u' {
            search_start = candidate + 1;
            continue;
        }
        break candidate;
    };
    let rest = &s[sec_start + 6..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse::<i64>().ok().map(|secs| secs * 1000)
}

// Parses `last -1 -F shutdown` output and returns the shutdown time in milliseconds.
// The `-F` flag on macOS BSD last includes the year and seconds, e.g.:
//   "shutdown  ~          Mon Jun 10 22:45:00 2024"
// Returns None if no shutdown line is found or the date is unparseable.
fn parse_last_shutdown_mac(s: &str) -> Option<i64> {
    use chrono::{Local, NaiveDateTime, TimeZone};

    for line in s.lines() {
        if !line.starts_with("shutdown") {
            continue;
        }
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
    }
    None
}

#[derive(Clone)]
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

    fn get_last_shutdown_time_utc_ms(&self) -> CoreResult<Option<i64>> {
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

    fn get_last_startup_time_utc_ms(&self) -> CoreResult<Option<i64>> {
        let output = Command::new("sysctl")
            .args(["-n", "kern.boottime"])
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
        Ok(parse_boottime_ms(&text))
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

impl PlatformHooks for MacPlatformHooks {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_boottime_ms_extracts_sec_from_sysctl_output() {
        let input = "{ sec = 1718000000, usec = 123456 } Tue Jun 10 12:00:00 2024\n";
        assert_eq!(parse_boottime_ms(input), Some(1_718_000_000_000));
    }

    #[test]
    fn parse_boottime_ms_returns_none_on_empty_input() {
        assert_eq!(parse_boottime_ms(""), None);
    }

    #[test]
    fn parse_boottime_ms_returns_none_when_sec_absent() {
        assert_eq!(parse_boottime_ms("{ usec = 123456 }"), None);
    }

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
}
