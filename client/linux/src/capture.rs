use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};

use crate::config::CaptureBackendHint;

#[derive(Clone, Copy, Debug)]
pub enum CaptureBackend {
    Wayland,
    X11,
}

#[derive(Clone, Debug)]
pub struct CaptureProbe {
    pub backend: Option<CaptureBackend>,
    pub captured_ok: bool,
    pub guidance: String,
}

pub fn detect_backend(hint: Option<CaptureBackendHint>) -> Option<CaptureBackend> {
    let wayland_available = env_var_nonempty("WAYLAND_DISPLAY").is_some();
    let x11_available = resolve_x11_display().is_some();

    if let Some(hint) = hint {
        match hint {
            CaptureBackendHint::Wayland if wayland_available => {
                return Some(CaptureBackend::Wayland);
            }
            CaptureBackendHint::X11 if x11_available => return Some(CaptureBackend::X11),
            _ => {}
        }
    }

    if wayland_available {
        return Some(CaptureBackend::Wayland);
    }

    if x11_available {
        return Some(CaptureBackend::X11);
    }

    None
}

pub fn probe_backend(hint: Option<CaptureBackendHint>) -> CaptureProbe {
    let backend = detect_backend(hint);

    match backend {
        Some(CaptureBackend::Wayland) => match capture_wayland() {
            Ok(_) => CaptureProbe {
                backend,
                captured_ok: true,
                guidance: "Wayland capture probe succeeded using grim.".to_string(),
            },
            Err(err) => CaptureProbe {
                backend,
                captured_ok: false,
                guidance: format!(
                    "Wayland detected but unattended capture failed: {}\nBest path: use an X11 session for headless capture reliability, or run a compositor that permits grim screencopy (for example sway/wlroots with correct permissions).",
                    err
                ),
            },
        },
        Some(CaptureBackend::X11) => match capture_x11() {
            Ok(_) => CaptureProbe {
                backend,
                captured_ok: true,
                guidance: "X11 capture probe succeeded.".to_string(),
            },
            Err(err) => CaptureProbe {
                backend,
                captured_ok: false,
                guidance: format!(
                    "X11 detected but capture failed: {}\nInstall one of these tools: ImageMagick (`import`) or `maim`, then rerun `bepure login`.",
                    err
                ),
            },
        },
        None => CaptureProbe {
            backend: None,
            captured_ok: false,
            guidance: "No graphical session detected. Run `bepure login` from a terminal inside your desktop session so capture permissions can be tested.".to_string(),
        },
    }
}

pub fn capture_screen(hint: Option<CaptureBackendHint>) -> Result<Vec<u8>> {
    match detect_backend(hint) {
        Some(CaptureBackend::Wayland) => capture_wayland(),
        Some(CaptureBackend::X11) => capture_x11(),
        None => Err(anyhow!(
            "no graphical session detected (missing WAYLAND_DISPLAY or DISPLAY)"
        )),
    }
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
