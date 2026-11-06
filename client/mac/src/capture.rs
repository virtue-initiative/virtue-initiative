use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use uuid::Uuid;
use virtue_core::{CoreError, CoreResult, LifecycleHooks, Screenshot, ScreenshotHooks};
use wait_timeout::ChildExt;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// The hidden `virtue-daemon` subcommand that answers whether Screen
/// Recording is granted, with exit code 0 or `CHECK_ACCESS_DENIED_EXIT`.
pub const CHECK_ACCESS_SUBCOMMAND: &str = "check-screen-capture";

/// The check's exit code when Screen Recording is off. Any other failure,
/// such as clap's 2 from a binary without the subcommand, means the check
/// itself failed.
pub const CHECK_ACCESS_DENIED_EXIT: u8 = 1;

/// How long a check may take before it counts as failed, so a hung check
/// can't stall a capture or the app's status poll.
pub const CHECK_ACCESS_TIMEOUT: Duration = Duration::from_secs(5);

/// What a new process said about Screen Recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessCheck {
    Granted,
    Denied,
    /// The check gave no answer: it failed to start, timed out or exited
    /// some other way. Holds the cause, for the logs.
    Failed(String),
}

pub fn capture_screen() -> Result<Vec<u8>> {
    match std::env::current_exe() {
        Ok(exe) => capture_screen_checked_by(&exe),
        Err(err) => capture_after_check(AccessCheck::Failed(format!(
            "cannot find own executable: {err}"
        ))),
    }
}

/// Captures the screen if `checker check-screen-capture` allows it.
/// `capture_screen` uses this executable, because the in-process check would
/// miss a grant removed since the daemon started, and `screencapture` would
/// then raise macOS's prompt and return only the desktop background
/// (CLIENT-025).
pub fn capture_screen_checked_by(checker: &Path) -> Result<Vec<u8>> {
    capture_after_check(check_access_in_new_process(checker, CHECK_ACCESS_TIMEOUT))
}

fn capture_after_check(check: AccessCheck) -> Result<Vec<u8>> {
    let granted = match check {
        AccessCheck::Granted => true,
        AccessCheck::Denied => false,
        // Most likely the app was moved or is mid-update, so the executable
        // isn't on disk. This process's own answer is only wrong if the
        // permission also changed since it started, which is far rarer than
        // refusing every capture until the daemon restarts.
        AccessCheck::Failed(cause) => {
            tracing::warn!(%cause, "Screen Recording check failed; using this process's own answer");
            has_screen_capture_access()
        }
    };
    if !granted {
        return Err(anyhow!(
            "screen recording permission missing (grant Screen Recording permission in macOS)"
        ));
    }

    run_capture_command("/usr/sbin/screencapture", &["-x", "-t", "png"])
        .or_else(|_| run_capture_command("screencapture", &["-x", "-t", "png"]))
        .with_context(|| "screencapture failed (grant Screen Recording permission in macOS)")
}

/// Whether this process may capture the screen. macOS answers once per
/// process, so in a long-running process this goes stale when the permission
/// changes (CLIENT-025). Only `virtue-daemon check-screen-capture` and the
/// launch-time request use it.
pub fn has_screen_capture_access() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Runs `program check-screen-capture`, which gets a fresh answer from macOS.
/// The app also calls this with the bundled daemon, to notice the permission
/// changing while it runs.
pub fn check_access_in_new_process(program: &Path, timeout: Duration) -> AccessCheck {
    let describe = || format!("{} {CHECK_ACCESS_SUBCOMMAND}", program.display());
    let mut child = match Command::new(program)
        .arg(CHECK_ACCESS_SUBCOMMAND)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => return AccessCheck::Failed(format!("cannot start {}: {err}", describe())),
    };
    let failure = match child.wait_timeout(timeout) {
        Ok(Some(status)) if status.success() => return AccessCheck::Granted,
        Ok(Some(status)) if status.code() == Some(i32::from(CHECK_ACCESS_DENIED_EXIT)) => {
            return AccessCheck::Denied;
        }
        Ok(Some(status)) => {
            return AccessCheck::Failed(format!("{} exited with {status}", describe()));
        }
        Ok(None) => format!("{} did not answer within {timeout:?}", describe()),
        Err(err) => format!("waiting for {} failed: {err}", describe()),
    };
    let _ = child.kill();
    let _ = child.wait();
    AccessCheck::Failed(failure)
}

/// Request screen capture access and return whether it is now granted.
pub fn request_screen_capture_access() -> bool {
    if has_screen_capture_access() {
        return true;
    }

    // Shows macOS's prompt, which also adds the app to the Screen Recording
    // list in System Settings. It returns immediately, before the user
    // answers. There used to be a throwaway `screencapture` here as well,
    // but that raised a second, identical prompt as soon as the first was
    // answered (issue #632).
    let _ = unsafe { CGRequestScreenCaptureAccess() };

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
    #[cfg(not(feature = "mock-capture"))]
    fn take_screenshot(&self) -> CoreResult<Screenshot> {
        let bytes = capture_screen().map_err(|err| {
            let message = err.to_string();
            if is_permission_missing_error(&message) {
                tracing::warn!(error = %message, "capture permission missing");
            } else {
                tracing::warn!(error = %message, "screenshot capture failed");
            }
            CoreError::CommandFailed(message)
        })?;
        Ok(Screenshot {
            captured_at_ms: self.get_time_utc_ms()?,
            bytes,
            content_type: "image/png".to_string(),
        })
    }

    // CI has no headless way to grant Screen Recording permission, so a real
    // `screencapture` always fails there (see `capture_screen`). This feature
    // swaps in a fixed embedded image instead, so CI can exercise the real
    // capture -> upload -> hash -> batch pipeline rather than only the
    // CaptureFailed alert path. Compiled in only when explicitly requested
    // (`cargo build --features mock-capture`) -- never by build-app.sh/
    // build-dmg.sh, so it cannot end up in a shipped build.
    #[cfg(feature = "mock-capture")]
    fn take_screenshot(&self) -> CoreResult<Screenshot> {
        Ok(Screenshot {
            captured_at_ms: self.get_time_utc_ms()?,
            bytes: include_bytes!("../assets/ci-mock-screenshot.png").to_vec(),
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
    /// Time since boot, in milliseconds, EXCLUDING time spent suspended.
    /// Feeds only `lifecycle::tick`'s suspend evidence (CORE-002).
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

#[cfg(test)]
// CLIENT-025: the daemon checks Screen Recording in a new process before
// each capture; only a clean denial means no access, and a denial means no
// capture.
mod permission_tests {
    use std::time::Instant;

    use super::*;

    fn check(program: &str) -> AccessCheck {
        check_access_in_new_process(Path::new(program), CHECK_ACCESS_TIMEOUT)
    }

    #[test]
    fn a_clean_exit_grants_and_the_denied_code_denies() {
        assert_eq!(check("/usr/bin/true"), AccessCheck::Granted);
        assert_eq!(check("/usr/bin/false"), AccessCheck::Denied);
    }

    #[test]
    fn a_check_that_cannot_answer_is_a_failure_not_a_denial() {
        // Missing executable, as when the app was moved while running.
        assert!(matches!(
            check("/nonexistent/virtue-daemon"),
            AccessCheck::Failed(_)
        ));
        // Like an older binary without the subcommand, `diff` with one
        // operand exits 2 for a usage error.
        assert!(matches!(check("/usr/bin/diff"), AccessCheck::Failed(_)));
    }

    #[test]
    fn a_check_that_never_exits_is_a_failure() {
        // `yes` ignores its argument and never exits on its own.
        let started = Instant::now();
        let result =
            check_access_in_new_process(Path::new("/usr/bin/yes"), Duration::from_millis(200));
        assert!(matches!(result, AccessCheck::Failed(_)), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_denied_check_fails_the_capture_as_a_permission_error() {
        let err =
            capture_screen_checked_by(Path::new("/usr/bin/false")).expect_err("capture must fail");
        assert!(is_permission_missing_error(&err.to_string()), "{err}");
    }
}

#[cfg(test)]
// CLIENT-018: macOS last login and logout times parsed from `last`.
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
