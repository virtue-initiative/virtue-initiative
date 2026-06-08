use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use virtue_core::{CoreError, CoreResult, PlatformHooks, ScreenshotHooks, Screenshot};

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

fn parse_btime_ms(proc_stat: &str) -> Option<i64> {
    for line in proc_stat.lines() {
        if let Some(rest) = line.strip_prefix("btime ") {
            return rest.trim().parse::<i64>().ok().map(|secs| secs * 1000);
        }
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

    fn get_last_shutdown_time_utc_ms(&self) -> CoreResult<Option<i64>> {
        Ok(None)
    }

    fn get_last_startup_time_utc_ms(&self) -> CoreResult<Option<i64>> {
        let stat = std::fs::read_to_string("/proc/stat")
            .map_err(|e| CoreError::CommandFailed(e.to_string()))?;
        Ok(parse_btime_ms(&stat))
    }
}

impl PlatformHooks for LinuxPlatformHooks {
    type CustomEvent = ();
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
    fn parse_btime_ms_extracts_boot_time_from_proc_stat() {
        let content = "cpu  1234 0 5678 9999\nbtime 1717200000\ncpu0 0 0 0 0\n";
        assert_eq!(parse_btime_ms(content), Some(1_717_200_000_000));
    }

    #[test]
    fn parse_btime_ms_returns_none_when_btime_absent() {
        let content = "cpu  1234 0 5678 9999\nprocs_running 1\n";
        assert_eq!(parse_btime_ms(content), None);
    }
}
