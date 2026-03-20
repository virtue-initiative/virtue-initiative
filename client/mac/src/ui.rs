use std::process::{Command, Output};

use anyhow::{Result, anyhow};

const LOGIN_SPLIT: &str = "__VIRTUE_SPLIT__";

#[derive(Debug, Clone)]
pub struct LoginInput {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoggedInAction {
    Close,
    RestartDaemon,
    Logout,
}

pub struct LoggedInDialogDetails<'a> {
    pub build_label: &'a str,
    pub email: &'a str,
    pub device_id: &'a str,
    pub monitor_summary: &'a str,
    pub pending_request_count: usize,
    pub base_api_url: &'a str,
    pub screenshot_permission: &'a str,
    pub daemon_status_updated_at: Option<&'a str>,
    pub daemon_last_error: Option<&'a str>,
}

pub fn prompt_login(build_label: &str, default_email: Option<&str>) -> Result<Option<LoginInput>> {
    let title = apple_script_escape(&format!("Virtue login ({build_label})"));
    let default_email = apple_script_escape(default_email.unwrap_or_default());
    let script = format!(
        r#"
set emailPrompt to display dialog "{title}" default answer "{default_email}" buttons {{"Cancel", "Next"}} default button "Next"
set emailValue to text returned of emailPrompt
set passwordPrompt to display dialog "Password" default answer "" with hidden answer buttons {{"Cancel", "Sign in"}} default button "Sign in"
set passwordValue to text returned of passwordPrompt
return emailValue & "__VIRTUE_SPLIT__" & passwordValue
"#
    );

    let Some(raw) = run_script_allow_cancel(&script)? else {
        return Ok(None);
    };

    let Some((email, password)) = raw.split_once(LOGIN_SPLIT) else {
        return Err(anyhow!("unexpected login dialog output"));
    };

    let email = email.trim().to_string();
    let password = password.to_string();
    if email.is_empty() || password.is_empty() {
        return Ok(None);
    }

    Ok(Some(LoginInput { email, password }))
}

pub fn prompt_logged_in_action(details: &LoggedInDialogDetails<'_>) -> Result<Option<LoggedInAction>> {
    let message = format!(
        "Version: {}\nSigned in as {}.\nDevice id: {}\nMonitoring: {}\nPending requests: {}\nAPI: {}\n\nDaemon status:\nScreen Recording permission: {}\nLast status update: {}\nLast daemon error: {}",
        details.build_label,
        details.email,
        details.device_id,
        details.monitor_summary,
        details.pending_request_count,
        details.base_api_url,
        details.screenshot_permission,
        details.daemon_status_updated_at.unwrap_or("<none>"),
        details
            .daemon_last_error
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("<none>")
    );
    let escaped = apple_script_escape(&message);
    let script = format!(
        r#"
set dialogResult to display dialog "{escaped}" buttons {{"Close", "Restart daemon", "Logout"}} default button "Close"
return button returned of dialogResult
"#
    );

    let Some(raw) = run_script_allow_cancel(&script)? else {
        return Ok(None);
    };

    Ok(parse_logged_in_action(&raw))
}

pub fn prompt_permission_issue_action(
    details: &LoggedInDialogDetails<'_>,
) -> Result<Option<LoggedInAction>> {
    let mut message = format!(
        "Version: {}\nSigned in as {}.\nDevice id: {}\nMonitoring: {}\nPending requests: {}\nAPI: {}\n\nDaemon status:\nScreen Recording permission: {}\nLast status update: {}\nLast daemon error: {}",
        details.build_label,
        details.email,
        details.device_id,
        details.monitor_summary,
        details.pending_request_count,
        details.base_api_url,
        details.screenshot_permission,
        details.daemon_status_updated_at.unwrap_or("<none>"),
        details
            .daemon_last_error
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("<none>")
    );
    message.push_str(
        "\n\nScreen Recording permission appears to be missing for the Virtue background service.\n\nOpen System Settings > Privacy & Security > Screen Recording, enable Virtue, then click Restart daemon. Restart is required even if you selected Quit & Reopen earlier.",
    );

    let escaped = apple_script_escape(&message);
    let script = format!(
        r#"
set dialogResult to display dialog "{escaped}" buttons {{"Close", "Restart daemon", "Logout"}} default button "Restart daemon" with icon caution
return button returned of dialogResult
"#
    );

    let Some(raw) = run_script_allow_cancel(&script)? else {
        return Ok(None);
    };

    Ok(parse_logged_in_action(&raw))
}

pub fn show_info(message: &str) -> Result<()> {
    let escaped = apple_script_escape(message);
    let script = format!(
        r#"display dialog "{escaped}" buttons {{"OK"}} default button "OK" with icon note"#
    );
    let _ = run_script_allow_cancel(&script)?;
    Ok(())
}

pub fn show_warning(message: &str) -> Result<()> {
    let escaped = apple_script_escape(message);
    let script = format!(
        r#"display dialog "{escaped}" buttons {{"OK"}} default button "OK" with icon caution"#
    );
    let _ = run_script_allow_cancel(&script)?;
    Ok(())
}

pub fn show_error(message: &str) -> Result<()> {
    let escaped = apple_script_escape(message);
    let script = format!(
        r#"display dialog "{escaped}" buttons {{"OK"}} default button "OK" with icon stop"#
    );
    let _ = run_script_allow_cancel(&script)?;
    Ok(())
}

fn run_script_allow_cancel(script: &str) -> Result<Option<String>> {
    let output = run_osascript(script)?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string();
        return Ok(Some(stdout));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("User canceled") {
        return Ok(None);
    }

    Err(anyhow!("osascript failed: {}", stderr.trim()))
}

fn run_osascript(script: &str) -> Result<Output> {
    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()?;
    Ok(output)
}

fn apple_script_escape(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', " ")
        .replace('\n', "\\n")
}

fn parse_logged_in_action(raw: &str) -> Option<LoggedInAction> {
    match raw.trim() {
        "Close" => Some(LoggedInAction::Close),
        "Restart daemon" => Some(LoggedInAction::RestartDaemon),
        "Logout" => Some(LoggedInAction::Logout),
        _ => None,
    }
}
