use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, anyhow};

use crate::config::ClientPaths;

pub const LABEL: &str = "codes.anb.bepure.daemon";

pub fn ensure_agent_running(paths: &ClientPaths, exe_path: &Path) -> Result<()> {
    let plist = render_plist(exe_path, paths);

    if let Some(parent) = paths.launch_agent_file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let needs_write = match fs::read_to_string(&paths.launch_agent_file) {
        Ok(existing) => existing != plist,
        Err(_) => true,
    };

    let uid = current_uid()?;
    let gui_domain = format!("gui/{uid}");
    let service_id = format!("{gui_domain}/{LABEL}");
    if needs_write {
        fs::write(&paths.launch_agent_file, plist).with_context(|| {
            format!(
                "failed writing launch agent {}",
                paths.launch_agent_file.display()
            )
        })?;
        let _ = run_launchctl(&["bootout", &service_id]);
    }

    let bootstrap_status = run_launchctl(&[
        "bootstrap",
        &gui_domain,
        &paths.launch_agent_file.display().to_string(),
    ])?;
    if !bootstrap_status.success {
        let lower = bootstrap_status.stderr.to_ascii_lowercase();
        if !lower.contains("service already loaded")
            && !lower.contains("already bootstrapped")
            && !lower.contains("in progress")
        {
            return Err(anyhow!(
                "launchctl bootstrap failed: {}",
                bootstrap_status.stderr.trim()
            ));
        }
    }

    let _ = run_launchctl(&["enable", &service_id]);
    let kickstart = run_launchctl(&["kickstart", "-k", &service_id])?;
    if !kickstart.success {
        return Err(anyhow!(
            "launchctl kickstart failed: {}",
            kickstart.stderr.trim()
        ));
    }

    Ok(())
}

struct LaunchctlOutput {
    success: bool,
    stderr: String,
}

fn run_launchctl(args: &[&str]) -> Result<LaunchctlOutput> {
    let output = Command::new("/bin/launchctl")
        .args(args)
        .output()
        .with_context(|| format!("failed to execute launchctl {}", args.join(" ")))?;

    Ok(LaunchctlOutput {
        success: output.status.success(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn current_uid() -> Result<String> {
    let output = Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .context("failed to resolve current uid")?;

    if !output.status.success() {
        return Err(anyhow!(
            "id -u failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn render_plist(exe_path: &Path, paths: &ClientPaths) -> String {
    let exe = xml_escape(&exe_path.display().to_string());
    let stdout_path = xml_escape(
        &paths
            .logs_dir
            .join("bepure-daemon.log")
            .display()
            .to_string(),
    );
    let stderr_path = xml_escape(
        &paths
            .logs_dir
            .join("bepure-daemon.error.log")
            .display()
            .to_string(),
    );

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>daemon</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{stdout_path}</string>
  <key>StandardErrorPath</key>
  <string>{stderr_path}</string>
</dict>
</plist>
"#
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
