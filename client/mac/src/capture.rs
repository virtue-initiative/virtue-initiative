use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};

pub fn capture_screen() -> Result<Vec<u8>> {
    run_capture_command("/usr/sbin/screencapture", &["-x", "-t", "png", "-"])
        .or_else(|_| run_capture_command("screencapture", &["-x", "-t", "png", "-"]))
        .with_context(|| "screencapture failed (grant Screen Recording permission in macOS)")
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
