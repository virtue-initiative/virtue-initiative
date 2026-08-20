use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use virtue_core::{CoreError, CoreResult, LifecycleHooks, Screenshot, ScreenshotHooks};

#[derive(Clone, Copy, Debug)]
pub enum CaptureBackend {
    Wayland,
    X11,
}

#[derive(Clone, Debug)]
pub struct CaptureProbe {
    pub captured_ok: bool,
    pub guidance: String,
}

pub fn detect_backend() -> Option<CaptureBackend> {
    detect_backend_from(
        env_var_nonempty("WAYLAND_DISPLAY").is_some(),
        resolve_x11_display().is_some(),
    )
}

fn detect_backend_from(wayland_available: bool, x11_available: bool) -> Option<CaptureBackend> {
    if wayland_available {
        return Some(CaptureBackend::Wayland);
    }
    if x11_available {
        return Some(CaptureBackend::X11);
    }
    None
}

pub fn probe_backend() -> CaptureProbe {
    let backend = detect_backend();

    match backend {
        Some(CaptureBackend::Wayland) => match capture_wayland() {
            Ok(_) => CaptureProbe {
                captured_ok: true,
                guidance: "Wayland capture probe succeeded using grim.".to_string(),
            },
            Err(err) => CaptureProbe {
                captured_ok: false,
                guidance: format!(
                    "Wayland detected but unattended capture failed: {}\nBest path: use an X11 session for headless capture reliability, or run a compositor that permits grim screencopy (for example sway/wlroots with correct permissions).",
                    err
                ),
            },
        },
        Some(CaptureBackend::X11) => match capture_x11() {
            Ok(_) => CaptureProbe {
                captured_ok: true,
                guidance: "X11 capture probe succeeded.".to_string(),
            },
            Err(err) => CaptureProbe {
                captured_ok: false,
                guidance: format!(
                    "X11 detected but capture failed: {}\nInstall one of these tools: ImageMagick (`import`) or `maim`, then rerun `virtue login`.",
                    err
                ),
            },
        },
        None => CaptureProbe {
            captured_ok: false,
            guidance: "No graphical session detected. Run `virtue login` from a terminal inside your desktop session so capture permissions can be tested.".to_string(),
        },
    }
}

pub fn capture_screen() -> Result<Vec<u8>> {
    match detect_backend() {
        Some(CaptureBackend::Wayland) => capture_wayland(),
        Some(CaptureBackend::X11) => capture_x11(),
        None => Err(anyhow!(
            "no graphical session detected (missing WAYLAND_DISPLAY or DISPLAY)"
        )),
    }
}

pub fn is_session_unavailable_text(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("no graphical session detected")
        || text.contains("x11 display unavailable")
        || text.contains("unable to open x server")
        || text.contains("can't open display")
}

fn capture_wayland() -> Result<Vec<u8>> {
    run_capture_command("grim", &["-"], &[]).with_context(
        || "grim capture failed (Wayland usually requires compositor support and permissions)",
    )
}

fn capture_x11() -> Result<Vec<u8>> {
    let display = resolve_x11_display().ok_or_else(|| {
        anyhow!("X11 display unavailable (DISPLAY unset and no /tmp/.X11-unix/X* socket found)")
    })?;
    let mut env_overrides = vec![("DISPLAY", display)];
    if let Some(xauthority) = resolve_xauthority() {
        env_overrides.push(("XAUTHORITY", xauthority));
    }

    let import_attempt =
        run_capture_command("import", &["-window", "root", "png:-"], &env_overrides);
    match import_attempt {
        Ok(bytes) => Ok(bytes),
        Err(import_error) => {
            let maim_attempt =
                run_capture_command("maim", &["-u", "-f", "png", "-"], &env_overrides);
            match maim_attempt {
                Ok(bytes) => Ok(bytes),
                Err(maim_error) => Err(anyhow!(
                    "import failed: {}; maim failed: {}",
                    import_error,
                    maim_error
                )),
            }
        }
    }
}

fn env_var_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_x11_display() -> Option<String> {
    env_var_nonempty("DISPLAY").or_else(detect_x11_socket_display)
}

fn detect_x11_socket_display() -> Option<String> {
    let mut display_numbers = Vec::new();
    let entries = std::fs::read_dir("/tmp/.X11-unix").ok()?;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(number) = name
            .strip_prefix('X')
            .and_then(|suffix| suffix.parse::<u32>().ok())
        {
            display_numbers.push(number);
        }
    }

    display_numbers.sort_unstable();
    display_numbers.first().map(|number| format!(":{number}"))
}

fn resolve_xauthority() -> Option<String> {
    env_var_nonempty("XAUTHORITY").or_else(|| {
        let home = env_var_nonempty("HOME")?;
        let path = std::path::Path::new(&home).join(".Xauthority");
        if path.exists() {
            Some(path.to_string_lossy().to_string())
        } else {
            None
        }
    })
}

fn run_capture_command(
    cmd: &str,
    args: &[&str],
    env_overrides: &[(&str, String)],
) -> Result<Vec<u8>> {
    let mut command = Command::new(cmd);
    command
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());

    for (key, value) in env_overrides {
        command.env(key, value);
    }

    let output = command
        .output()
        .with_context(|| format!("failed to execute {cmd}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("{} exited with {}: {}", cmd, output.status, stderr));
    }

    if output.stdout.is_empty() {
        return Err(anyhow!("{} returned empty output", cmd));
    }

    Ok(output.stdout)
}

// Parses `journalctl --list-boots -o json` output and returns the last-entry timestamp
// (µs → ms) of the previous boot (index == -1). Returns None if there is no previous boot,
// the output is unparseable, or journald has no persistent log. This is a floor/approximation
// of the true logout/shutdown time, not exact — see `LifecycleHooks::get_last_logout_utc_ms`.
fn parse_last_shutdown_ms(json: &str) -> Option<i64> {
    let entries: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;
    for entry in &entries {
        let Some(index) = entry.get("index").and_then(|v| v.as_i64()) else {
            continue;
        };
        if index != -1 {
            continue;
        }
        let last_entry = entry.get("last_entry")?;
        let us = last_entry
            .as_i64()
            .or_else(|| last_entry.as_str()?.parse::<i64>().ok())?;
        return Some(us / 1000);
    }
    None
}

#[derive(Clone)]
pub struct LinuxPlatformHooks;

impl LinuxPlatformHooks {
    pub fn new() -> Self {
        Self
    }
}

impl ScreenshotHooks for LinuxPlatformHooks {
    fn take_screenshot(&self) -> CoreResult<Screenshot> {
        let bytes = capture_screen().map_err(|err| {
            let message = err.to_string();
            if is_session_unavailable_text(&message) {
                // Common/expected while no graphical session is active (e.g. before
                // login) — the capture cadence is now on the order of minutes, so this
                // no longer needs throttled logging.
                tracing::debug!(error = %message, "capture session unavailable");
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

    fn is_locked_or_screensaver(&self) -> CoreResult<bool> {
        // Prefer logind's per-session `LockedHint`: the locker reports it across desktops
        // (xss-lock, GNOME, KDE, …), so it covers minimal X11 setups like i3 + i3lock that
        // expose no ScreenSaver D-Bus interface at all. Fall back to the freedesktop/GNOME
        // ScreenSaver `GetActive` for environments that surface a screensaver but no
        // LockedHint. Any error / missing service ⇒ false (fall back to the diff gate),
        // never silently suppress.
        if query_session_locked() == Some(true) {
            return Ok(true);
        }
        Ok(query_screensaver_active().unwrap_or(false))
    }
}

// Blocking logind proxies (system bus). `LockedHint` lives on the per-session object, so we
// resolve the caller's primary graphical session via the user object's `Display` property
// (the same path `loginctl` uses) and read the hint there.
#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1",
    gen_async = false,
    gen_blocking = true
)]
trait Login1Manager {
    fn get_user(&self, uid: u32) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.User",
    default_service = "org.freedesktop.login1",
    gen_async = false,
    gen_blocking = true
)]
trait Login1User {
    /// `(session_id, object_path)` of the user's primary graphical session.
    #[zbus(property)]
    fn display(&self) -> zbus::Result<(String, zbus::zvariant::OwnedObjectPath)>;
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1",
    gen_async = false,
    gen_blocking = true
)]
trait Login1Session {
    #[zbus(property)]
    fn locked_hint(&self) -> zbus::Result<bool>;
    /// Session start time, µs since epoch.
    #[zbus(property)]
    fn timestamp(&self) -> zbus::Result<u64>;
}

/// Real UID of this process, read from `/proc/self/status` (no libc dependency).
fn current_uid() -> Option<u32> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix("Uid:")
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|uid| uid.parse().ok())
    })
}

/// Resolve the caller's primary graphical session via the user object's
/// `Display` property (the same path `loginctl` uses).
fn resolve_primary_session<'c>(
    connection: &'c zbus::blocking::Connection,
) -> Option<Login1SessionProxy<'c>> {
    let manager = Login1ManagerProxy::new(connection).ok()?;
    let user_path = manager.get_user(current_uid()?).ok()?;
    let user = Login1UserProxy::builder(connection)
        .path(user_path)
        .ok()?
        .build()
        .ok()?;
    let (_session_id, session_path) = user.display().ok()?;
    Login1SessionProxy::builder(connection)
        .path(session_path)
        .ok()?
        .build()
        .ok()
}

/// `Some(true)`/`Some(false)` if logind reports the primary graphical session's lock state,
/// `None` if logind is unavailable or the session can't be resolved (⇒ fall back to the
/// screensaver query).
fn query_session_locked() -> Option<bool> {
    let connection = zbus::blocking::Connection::system().ok()?;
    let session = resolve_primary_session(&connection)?;
    session.locked_hint().ok()
}

/// The primary graphical session's start time (logind's `Timestamp` property,
/// µs since epoch, converted to ms), or `None` if logind is unavailable or the
/// session can't be resolved.
fn query_session_login_time_ms() -> Option<i64> {
    let connection = zbus::blocking::Connection::system().ok()?;
    let session = resolve_primary_session(&connection)?;
    session.timestamp().ok().map(|us| (us / 1000) as i64)
}

// Blocking D-Bus proxies for the two common `GetActive()` providers. We use the
// blocking flavour (gen_async = false) because `is_locked_or_screensaver` is a
// synchronous hook called from the event bus.
#[zbus::proxy(
    interface = "org.freedesktop.ScreenSaver",
    default_service = "org.freedesktop.ScreenSaver",
    default_path = "/org/freedesktop/ScreenSaver",
    gen_async = false,
    gen_blocking = true
)]
trait FreedesktopScreenSaver {
    fn get_active(&self) -> zbus::Result<bool>;
}

#[zbus::proxy(
    interface = "org.gnome.ScreenSaver",
    default_service = "org.gnome.ScreenSaver",
    default_path = "/org/gnome/ScreenSaver",
    gen_async = false,
    gen_blocking = true
)]
trait GnomeScreenSaver {
    fn get_active(&self) -> zbus::Result<bool>;
}

fn query_screensaver_active() -> Option<bool> {
    let connection = zbus::blocking::Connection::session().ok()?;

    if let Ok(proxy) = FreedesktopScreenSaverProxy::new(&connection)
        && let Ok(active) = proxy.get_active()
    {
        return Some(active);
    }

    if let Ok(proxy) = GnomeScreenSaverProxy::new(&connection)
        && let Ok(active) = proxy.get_active()
    {
        return Some(active);
    }

    None
}

impl LifecycleHooks for LinuxPlatformHooks {
    // `CLOCK_MONOTONIC` excludes time spent suspended (unlike
    // `CLOCK_BOOTTIME`, which includes it) — see `clock_gettime(2)`. Feeds
    // only `lifecycle::tick`'s suspend evidence (`SPEC.md` §2); screenshot
    // scheduling is unaffected.
    fn get_monotonic_clock_ms(&self) -> CoreResult<i64> {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `ts` is a valid, owned out-param that does not outlive this
        // call; `clock_gettime` has no other preconditions.
        let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
        if rc != 0 {
            return Err(CoreError::CommandFailed(format!(
                "clock_gettime(CLOCK_MONOTONIC) failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(ts.tv_sec * 1000 + ts.tv_nsec / 1_000_000)
    }

    fn get_last_login_utc_ms(&self) -> CoreResult<Option<i64>> {
        Ok(query_session_login_time_ms())
    }

    fn get_last_logout_utc_ms(&self) -> CoreResult<Option<i64>> {
        let output = Command::new("journalctl")
            .args(["--list-boots", "-o", "json"])
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
        let json = String::from_utf8_lossy(&output.stdout);
        Ok(parse_last_shutdown_ms(&json))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use virtue_core::ScreenshotHooks;

    #[test]
    fn is_session_unavailable_text_matches_known_error_strings() {
        assert!(is_session_unavailable_text("no graphical session detected"));
        assert!(is_session_unavailable_text("X11 display unavailable"));
        assert!(is_session_unavailable_text("unable to open X server"));
        assert!(is_session_unavailable_text("can't open display"));
    }

    #[test]
    fn is_session_unavailable_text_rejects_unrelated_errors() {
        assert!(!is_session_unavailable_text("permission denied"));
        assert!(!is_session_unavailable_text("command not found: grim"));
        assert!(!is_session_unavailable_text(""));
        assert!(!is_session_unavailable_text("out of memory"));
    }

    #[test]
    fn detect_backend_returns_wayland_when_wayland_available() {
        assert!(matches!(
            detect_backend_from(true, false),
            Some(CaptureBackend::Wayland)
        ));
    }

    #[test]
    fn detect_backend_prefers_wayland_over_x11() {
        assert!(matches!(
            detect_backend_from(true, true),
            Some(CaptureBackend::Wayland)
        ));
    }

    #[test]
    fn detect_backend_returns_x11_when_only_x11_available() {
        assert!(matches!(
            detect_backend_from(false, true),
            Some(CaptureBackend::X11)
        ));
    }

    #[test]
    fn detect_backend_returns_none_when_no_display() {
        assert!(detect_backend_from(false, false).is_none());
    }

    #[test]
    fn platform_hooks_get_time_utc_ms_is_positive() {
        let hooks = LinuxPlatformHooks::new();
        let ms = hooks.get_time_utc_ms().expect("clock should not fail");
        assert!(ms > 0);
    }

    #[test]
    fn platform_hooks_get_monotonic_clock_ms_is_positive_and_advances() {
        let hooks = LinuxPlatformHooks::new();
        let first = hooks
            .get_monotonic_clock_ms()
            .expect("clock should not fail");
        assert!(first > 0);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = hooks
            .get_monotonic_clock_ms()
            .expect("clock should not fail");
        assert!(second >= first);
    }

    #[test]
    fn parse_last_shutdown_ms_extracts_previous_boot_last_entry() {
        let json = r#"[
            {"index":-1,"boot_id":"aaa","first_entry":1717200000000000,"last_entry":1717286400000000},
            {"index":0,"boot_id":"bbb","first_entry":1717300000000000,"last_entry":1717310000000000}
        ]"#;
        assert_eq!(parse_last_shutdown_ms(json), Some(1_717_286_400_000));
    }

    #[test]
    fn parse_last_shutdown_ms_handles_string_timestamps() {
        let json = r#"[
            {"index":-1,"boot_id":"aaa","first_entry":"1717200000000000","last_entry":"1717286400000000"},
            {"index":0,"boot_id":"bbb","first_entry":"1717300000000000","last_entry":"1717310000000000"}
        ]"#;
        assert_eq!(parse_last_shutdown_ms(json), Some(1_717_286_400_000));
    }

    #[test]
    fn parse_last_shutdown_ms_returns_none_when_only_current_boot() {
        let json = r#"[{"index":0,"boot_id":"bbb","first_entry":1717300000000000,"last_entry":1717310000000000}]"#;
        assert_eq!(parse_last_shutdown_ms(json), None);
    }

    #[test]
    fn parse_last_shutdown_ms_returns_none_on_empty_array() {
        assert_eq!(parse_last_shutdown_ms("[]"), None);
    }

    #[test]
    fn parse_last_shutdown_ms_returns_none_on_invalid_json() {
        assert_eq!(parse_last_shutdown_ms("not json"), None);
        assert_eq!(parse_last_shutdown_ms(""), None);
    }
}
