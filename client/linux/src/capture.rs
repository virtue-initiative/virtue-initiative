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
    if let Some(hint) = hint {
        return Some(match hint {
            CaptureBackendHint::Wayland => CaptureBackend::Wayland,
            CaptureBackendHint::X11 => CaptureBackend::X11,
        });
    }

    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        return Some(CaptureBackend::Wayland);
    }

    if std::env::var("DISPLAY").is_ok() {
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
    run_capture_command("grim", &["-"]).with_context(
        || "grim capture failed (Wayland usually requires compositor support and permissions)",
    )
}

fn capture_x11() -> Result<Vec<u8>> {
    let import_attempt = run_capture_command("import", &["-window", "root", "png:-"]);
    match import_attempt {
        Ok(bytes) => Ok(bytes),
        Err(import_error) => {
            let maim_attempt = run_capture_command("maim", &["-u", "-f", "png", "-"]);
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

fn run_capture_command(cmd: &str, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
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
