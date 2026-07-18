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

// Parses a single `last -y -s` line's leading date (e.g.
// "shutdown time                              Sat Jul  4 2026 18:23") into
// milliseconds since epoch. Unlike Linux's util-linux `last`, macOS's BSD
// `last` has no `-F`/full-format flag at all (confirmed against `man last`
// on macOS 15); `-y` is required to get the year at all (default output omits
// it, making it ambiguous), and `-s` (used by `parse_last_session_duration_secs`)
// switches session durations to plain integer seconds instead of a
// `[D+]HH:MM[:SS]` string. There's still no flag that adds seconds to the
// start time itself, so precision here bottoms out around a minute — well
// under `PER_GAP_THRESHOLD_MS` (measured in tens of seconds), an acceptable
// floor. Returns None if the date is unparseable.
fn parse_last_line_date(line: &str) -> Option<i64> {
    use chrono::{Local, NaiveDateTime, TimeZone};

    let tokens: Vec<&str> = line.split_whitespace().collect();
    for w in tokens.windows(5) {
        let (Ok(day), Ok(_year)) = (w[2].parse::<u32>(), w[3].parse::<i32>()) else {
            continue;
        };
        let normalized = format!("{} {} {:02} {} {}", w[0], w[1], day, w[4], w[3]);
        if let Ok(dt) = NaiveDateTime::parse_from_str(&normalized, "%a %b %d %H:%M %Y")
            && let chrono::LocalResult::Single(local) = Local.from_local_datetime(&dt)
        {
            return Some(local.timestamp_millis());
        }
    }
    None
}

// Parses the integer-second duration from a `last -s` session line's
// trailing "( N )" group, e.g. "... Sat Jul  4 2026 18:24 - 22:32  ( 1138099)"
// (this also covers sessions `last` marks "- crash" or "- shutdown" instead
// of a clean end time — the numeric duration is still present either way).
// Returns None for still-active sessions, which have no trailing group.
fn parse_last_session_duration_secs(line: &str) -> Option<i64> {
    let open = line.rfind('(')?;
    let close = open + line[open..].find(')')?;
    line[open + 1..close].trim().parse::<i64>().ok()
}

// Parses `last -y -s` output and returns the most recent known session end
// (logout) in milliseconds. On macOS this can come from two different kinds
// of line: a "shutdown time" pseudo-entry, logged only when the *machine*
// actually powers off/reboots; or an ordinary completed session line's own
// start time plus duration, which is the *only* record of a plain "Log Out"
// that returns to the login window without powering the machine off — macOS
// never logs a separate event for that case. Returns the max of every
// candidate found in the window, i.e. the most recent of either kind. This is
// a floor/approximation of the true logout time, not exact — see
// `LifecycleHooks::get_last_logout_utc_ms`.
fn parse_last_logout_mac(s: &str) -> Option<i64> {
    s.lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("shutdown") {
                return parse_last_line_date(line);
            }
            if trimmed.is_empty()
                || trimmed.starts_with("reboot")
                || trimmed.starts_with("wtmp begins")
            {
                return None;
            }
            let start_ms = parse_last_line_date(line)?;
            let duration_secs = parse_last_session_duration_secs(line)?;
            Some(start_ms + duration_secs * 1000)
        })
        .max()
}

// Parses unfiltered `last -y -s` output and returns the most recent real
// login's timestamp in milliseconds, skipping the "reboot"/"shutdown"
// pseudo-user entries `last` also logs.
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
            .args(["-y", "-s", "-n", "5"])
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
        // `-n 15`: enough history to reliably see a shutdown/crash line even
        // when several plain logouts (which don't get their own separate
        // entry — see `parse_last_logout_mac`) preceded it.
        let output = Command::new("last")
            .args(["-y", "-s", "-n", "15"])
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
        Ok(parse_last_logout_mac(&text))
    }
}

impl PlatformHooks for MacPlatformHooks {}

#[cfg(test)]
mod tests {
    use chrono::{Local, TimeZone};

    use super::*;

    // `-y` gives an explicit year, so test fixtures can use fixed dates —
    // chrono derives the correct weekday for us, and validates it against
    // the date when parsing (same guard real macOS `last` output gets).
    fn fixed_dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> chrono::DateTime<Local> {
        Local
            .with_ymd_and_hms(y, mo, d, h, mi, 0)
            .single()
            .expect("valid fixed timestamp")
    }

    // macOS `last -y`'s date format: weekday, month, space-padded day, year, HH:MM — still no seconds.
    fn fmt_last_line(dt: chrono::DateTime<Local>) -> String {
        dt.format("%a %b %e %Y %H:%M").to_string()
    }

    #[test]
    fn parse_last_session_duration_secs_extracts_trailing_group() {
        assert_eq!(
            parse_last_session_duration_secs("jeff console ... - 22:32  ( 1138099)"),
            Some(1_138_099)
        );
        assert_eq!(
            parse_last_session_duration_secs("jeff console ... - 21:34  (       0)"),
            Some(0)
        );
    }

    #[test]
    fn parse_last_session_duration_secs_returns_none_for_still_active_session() {
        assert_eq!(
            parse_last_session_duration_secs("jeff console ... 22:33   still logged in"),
            None
        );
    }

    #[test]
    fn parse_last_logout_mac_extracts_timestamp_from_shutdown_line() {
        let when = fixed_dt(2024, 6, 10, 22, 45);
        let input = format!(
            "shutdown time                              {}\n\nwtmp begins Mon Jun  3 08:00\n",
            fmt_last_line(when)
        );
        assert_eq!(parse_last_logout_mac(&input), Some(when.timestamp_millis()));
    }

    #[test]
    fn parse_last_logout_mac_extracts_timestamp_from_plain_logout_with_no_shutdown_line() {
        // A plain "Log Out" (return to login window, machine stays powered
        // on) never logs a "shutdown time" entry on macOS — it's only
        // recoverable as the completed session's own start + duration. This
        // is the scenario a real logout/login cycle exercises.
        let start = fixed_dt(2026, 7, 4, 18, 24);
        let duration_secs = 1_138_099; // 13d 4h 8m 19s
        let input = format!(
            "jeff       console                         {} - 22:32  ({duration_secs})\nreboot time                                {}\n",
            fmt_last_line(start),
            fmt_last_line(start),
        );
        let expected = start.timestamp_millis() + duration_secs * 1000;
        assert_eq!(parse_last_logout_mac(&input), Some(expected));
    }

    #[test]
    fn parse_last_logout_mac_picks_most_recent_of_shutdown_and_plain_logout_candidates() {
        let older_shutdown = fixed_dt(2024, 6, 1, 9, 0);
        let newer_session_start = fixed_dt(2024, 6, 10, 22, 45);
        let newer_duration_secs = 900; // ends 2024-06-10 23:00
        let input = format!(
            "shutdown time                              {}\nalice     console                         {} - 23:00  ({newer_duration_secs})\n",
            fmt_last_line(older_shutdown),
            fmt_last_line(newer_session_start),
        );
        let expected = newer_session_start.timestamp_millis() + newer_duration_secs * 1000;
        assert_eq!(parse_last_logout_mac(&input), Some(expected));
    }

    #[test]
    fn parse_last_logout_mac_ignores_still_active_and_reboot_lines() {
        let input = "jeff       console                         Fri Jul 17 2026 22:33   still logged in\nreboot time                                Fri Jul 17 2026 22:33\n\nwtmp begins Mon Jun  3 08:00\n";
        assert_eq!(parse_last_logout_mac(input), None);
    }

    #[test]
    fn parse_last_logout_mac_returns_none_on_empty_input() {
        assert_eq!(parse_last_logout_mac(""), None);
    }

    #[test]
    fn parse_last_login_mac_extracts_most_recent_real_login() {
        let when = fixed_dt(2024, 6, 10, 22, 45);
        let input = format!(
            "alice     console                         {} - 23:00   (00:15)\n\nwtmp begins Mon Jun  3 08:00\n",
            fmt_last_line(when)
        );
        let result = parse_last_login_mac(&input);
        assert_eq!(result, Some(when.timestamp_millis()));
    }

    #[test]
    fn parse_last_login_mac_skips_reboot_and_shutdown_pseudo_entries() {
        let reboot_when = fixed_dt(2024, 6, 10, 22, 0);
        let login_when = fixed_dt(2024, 6, 10, 21, 45);
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
        let reboot_when = fixed_dt(2024, 6, 10, 22, 0);
        let shutdown_when = fixed_dt(2024, 6, 10, 21, 0);
        let input = format!(
            "reboot time                                {}\nshutdown time                              {}\n\nwtmp begins Mon Jun  3 08:00\n",
            fmt_last_line(reboot_when),
            fmt_last_line(shutdown_when)
        );
        assert_eq!(parse_last_login_mac(&input), None);
    }
}
