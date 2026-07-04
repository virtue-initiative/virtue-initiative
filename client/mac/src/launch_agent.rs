use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};

use crate::config::ClientPaths;

pub const LABEL: &str = "org.virtueinitiative.virtue.daemon";

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

    // A disabled override causes bootstrap to fail with a generic I/O error.
    // Ensure the service is enabled before attempting to load the agent.
    let _ = run_launchctl(&["enable", &service_id]);

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
        // launchctl can return a generic bootstrap failure even when the service
        // is already loaded. Treat that state as success.
        if !service_is_loaded(&service_id)? {
            return Err(anyhow!(
                "launchctl bootstrap failed: {}",
                bootstrap_status.describe_failure()
            ));
        }
    }

    let _ = run_launchctl(&["enable", &service_id]);
    let kickstart = run_launchctl(&["kickstart", "-k", &service_id])?;
    if !kickstart.success {
        return Err(anyhow!(
            "launchctl kickstart failed: {}",
            kickstart.describe_failure()
        ));
    }

    Ok(())
}

pub fn stop_agent(paths: &ClientPaths) -> Result<()> {
    let uid = current_uid()?;
    let gui_domain = format!("gui/{uid}");
    let service_id = format!("{gui_domain}/{LABEL}");
    let plist_path = paths.launch_agent_file.display().to_string();
    let mut failures = Vec::new();

    let by_service = run_launchctl(&["bootout", &service_id])?;
    if !by_service.success && service_is_loaded(&service_id)? {
        let by_plist = run_launchctl(&["bootout", &gui_domain, &plist_path])?;
        if !by_plist.success && wait_for_loaded_state(&service_id, false, STOP_WAIT_TIMEOUT)? {
            failures.push(format!(
                "failed to unload launch agent: {}; {}",
                by_service.describe_failure(),
                by_plist.describe_failure()
            ));
        }
    }

    let disable = run_launchctl(&["disable", &service_id])?;
    if !disable.success
        && !is_missing_target_error(&format!("{} {}", disable.stdout, disable.stderr))
    {
        failures.push(format!(
            "failed to disable launch agent override: {}",
            disable.describe_failure()
        ));
    }

    match fs::remove_file(&paths.launch_agent_file) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => failures.push(format!(
            "failed to remove launch agent plist {}: {err}",
            paths.launch_agent_file.display()
        )),
    }

    if wait_for_loaded_state(&service_id, false, STOP_WAIT_TIMEOUT)? {
        failures.push("launch agent is still loaded after stop".to_string());
    }

    if paths.launch_agent_file.exists() {
        failures.push(format!(
            "launch agent plist still exists at {}",
            paths.launch_agent_file.display()
        ));
    }

    if failures.is_empty() {
        return Ok(());
    }

    Err(anyhow!(failures.join("; ")))
}

pub fn is_agent_loaded() -> Result<bool> {
    let uid = current_uid()?;
    let service_id = format!("gui/{uid}/{LABEL}");
    service_is_loaded(&service_id)
}

struct LaunchctlOutput {
    success: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl LaunchctlOutput {
    /// `launchctl` often reports failures on stdout (e.g. plain "Bad
    /// request." from `kickstart`) rather than stderr, and can exit nonzero
    /// with both streams empty. Combine everything available so callers
    /// never surface a blank error message.
    fn describe_failure(&self) -> String {
        let stdout = self.stdout.trim();
        let stderr = self.stderr.trim();
        let mut parts = Vec::new();
        if !stdout.is_empty() {
            parts.push(format!("stdout: {stdout}"));
        }
        if !stderr.is_empty() {
            parts.push(format!("stderr: {stderr}"));
        }
        if parts.is_empty() {
            parts.push(format!(
                "exit code {}, no output",
                self.exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ));
        }
        parts.join("; ")
    }
}

fn run_launchctl(args: &[&str]) -> Result<LaunchctlOutput> {
    let output = Command::new("/bin/launchctl")
        .args(args)
        .output()
        .with_context(|| format!("failed to execute launchctl {}", args.join(" ")))?;

    Ok(LaunchctlOutput {
        success: output.status.success(),
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn service_is_loaded(service_id: &str) -> Result<bool> {
    let output = Command::new("/bin/launchctl")
        .args(["print", service_id])
        .output()
        .with_context(|| format!("failed to execute launchctl print {service_id}"))?;
    Ok(output.status.success())
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

fn is_missing_target_error(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    lower.contains("could not find service")
        || lower.contains("could not find specified service")
        || lower.contains("service not found")
        || lower.contains("no such process")
        || lower.contains("no such file")
        || lower.contains("not loaded")
}

const STOP_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(50);

fn wait_for_loaded_state(
    service_id: &str,
    expected_loaded: bool,
    timeout: Duration,
) -> Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        let is_loaded = service_is_loaded(service_id)?;
        if is_loaded == expected_loaded {
            return Ok(is_loaded);
        }
        if Instant::now() >= deadline {
            return Ok(is_loaded);
        }
        thread::sleep(STOP_WAIT_POLL_INTERVAL);
    }
}

fn render_plist(exe_path: &Path, paths: &ClientPaths) -> String {
    let exe = xml_escape(&exe_path.display().to_string());
    let stdout_path = xml_escape(
        &paths
            .logs_dir
            .join("virtue-daemon.log")
            .display()
            .to_string(),
    );
    let stderr_path = xml_escape(
        &paths
            .logs_dir
            .join("virtue-daemon.error.log")
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
