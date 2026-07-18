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

// Parses a single `last` line's trailing date (e.g.
// "shutdown time                              Sat Jul  4 18:23") into
// milliseconds since epoch. Unlike Linux's util-linux `last`, macOS's BSD
// `last` has no `-F`/full-format flag at all (confirmed against `man last`
// on macOS 15) — its default output carries only weekday/month/day/HH:MM,
// with no year and no seconds. We recover the year by assuming the entry is
// from the current year, falling back to the previous year if that would
// place it in the future (`last` never lists future entries) — this only
// picks the wrong year in the pathological case of a >1-year-old, otherwise
// ambiguous entry, which is far outside the gap-detection windows this data
// feeds. Seconds are unavailable and default to :00; this bounds precision
// to well under a minute, an acceptable floor given `PER_GAP_THRESHOLD_MS`
// is measured in tens of seconds. Returns None if the date is unparseable.
fn parse_last_line_date(line: &str) -> Option<i64> {
    use chrono::{Datelike, Local, NaiveDateTime, TimeZone};

    let now = Local::now();
    let tokens: Vec<&str> = line.split_whitespace().collect();
    for w in tokens.windows(4) {
        let Ok(day) = w[2].parse::<u32>() else {
            continue;
        };
        for year in [now.year(), now.year() - 1] {
            let normalized = format!("{} {} {:02} {} {}", w[0], w[1], day, w[3], year);
            if let Ok(dt) = NaiveDateTime::parse_from_str(&normalized, "%a %b %d %H:%M %Y")
                && let chrono::LocalResult::Single(local) = Local.from_local_datetime(&dt)
                && local <= now
            {
                return Some(local.timestamp_millis());
            }
        }
    }
    None
}

// Parses `last reboot` output (which, on macOS, interleaves "reboot time" and
// "shutdown time" pseudo-user entries — filtering by the literal username
// `shutdown` matches nothing on macOS's `last`) and returns the most recent
// shutdown time in milliseconds. Returns None if no shutdown line is found or
// the date is unparseable. This is a floor/approximation of the true
// logout/shutdown time, not exact — see `LifecycleHooks::get_last_logout_utc_ms`.
fn parse_last_shutdown_mac(s: &str) -> Option<i64> {
    s.lines()
        .find(|line| line.trim_start().starts_with("shutdown"))
        .and_then(parse_last_line_date)
}

// Parses unfiltered `last` output and returns the most recent real login's
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
            .args(["-n", "5"])
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
        // `reboot` (not `shutdown`) is the filter that actually works here —
        // see `parse_last_shutdown_mac`. `-n 10` gives enough history to find
        // a shutdown line even right after a fresh boot with few prior pairs.
        let output = Command::new("last")
            .args(["-n", "10", "reboot"])
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
    use chrono::{Datelike, Duration, Local, TimeZone, Timelike};

    use super::*;

    /// A timestamp `hours_ago` in the past, truncated to whole minutes —
    /// real macOS `last` output carries no seconds, so round-tripping through
    /// the parser should reproduce this exactly. Deriving it from `Local::now()`
    /// (rather than a fixed date) keeps the weekday chrono computes for the
    /// formatted line consistent with whatever year the parser's own
    /// `Local::now()` call lands on when the test runs.
    fn recent_minute(hours_ago: i64) -> chrono::DateTime<Local> {
        let now = Local::now()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();
        now - Duration::hours(hours_ago)
    }

    fn fmt_last_line(dt: chrono::DateTime<Local>) -> String {
        // macOS `last`'s real default format: weekday, month, space-padded day, HH:MM — no year, no seconds.
        dt.format("%a %b %e %H:%M").to_string()
    }

    #[test]
    fn parse_last_shutdown_mac_extracts_timestamp_from_last_output() {
        let when = recent_minute(2);
        let input = format!(
            "shutdown time                              {}\n\nwtmp begins Mon Jul  1 09:00\n",
            fmt_last_line(when)
        );
        let result = parse_last_shutdown_mac(&input);
        assert_eq!(result, Some(when.timestamp_millis()));
    }

    #[test]
    fn parse_last_shutdown_mac_handles_single_digit_day() {
        // Pick an instant that's guaranteed to land on a single-digit day of
        // the month, so `%e`'s space padding ("Jul  4" not "Jul 14") is
        // exercised the same way real `last` output is.
        let now = Local::now();
        let first_of_month = Local
            .with_ymd_and_hms(now.year(), now.month(), 1, 9, 0, 0)
            .single()
            .expect("valid first-of-month timestamp");
        let when = if first_of_month <= now {
            first_of_month
        } else {
            first_of_month - Duration::days(28)
        };
        let input = format!(
            "shutdown time                              {}\n",
            fmt_last_line(when)
        );
        let result = parse_last_shutdown_mac(&input);
        assert_eq!(result, Some(when.timestamp_millis()));
    }

    #[test]
    fn parse_last_shutdown_mac_returns_none_when_no_shutdown_line() {
        let when = recent_minute(2);
        let input = format!(
            "reboot time                                {}\n\nwtmp begins Mon Jul  1 09:00\n",
            fmt_last_line(when)
        );
        assert_eq!(parse_last_shutdown_mac(&input), None);
    }

    #[test]
    fn parse_last_shutdown_mac_returns_none_on_empty_input() {
        assert_eq!(parse_last_shutdown_mac(""), None);
    }

    #[test]
    fn parse_last_login_mac_extracts_most_recent_real_login() {
        let when = recent_minute(3);
        let input = format!(
            "alice     console                         {} - 23:00   (00:15)\n\nwtmp begins Mon Jul  1 09:00\n",
            fmt_last_line(when)
        );
        let result = parse_last_login_mac(&input);
        assert_eq!(result, Some(when.timestamp_millis()));
    }

    #[test]
    fn parse_last_login_mac_skips_reboot_and_shutdown_pseudo_entries() {
        let reboot_when = recent_minute(5);
        let login_when = recent_minute(4);
        let input = format!(
            "reboot time                                {}\nalice     console                         {} - 22:00   (00:15)\n",
            fmt_last_line(reboot_when),
            fmt_last_line(login_when)
        );
        let result = parse_last_login_mac(&input);
        assert_eq!(result, Some(login_when.timestamp_millis()));
    }

    #[test]
    fn parse_last_login_mac_returns_none_when_only_pseudo_entries() {
        let reboot_when = recent_minute(5);
        let shutdown_when = recent_minute(6);
        let input = format!(
            "reboot time                                {}\nshutdown time                              {}\n\nwtmp begins Mon Jul  1 09:00\n",
            fmt_last_line(reboot_when),
            fmt_last_line(shutdown_when)
        );
        assert_eq!(parse_last_login_mac(&input), None);
    }
}
