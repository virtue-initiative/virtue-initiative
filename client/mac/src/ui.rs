use std::process::{Command, Output};

use anyhow::{Result, anyhow};

const LOGIN_SPLIT: &str = "__BEPURE_SPLIT__";

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

pub fn prompt_login() -> Result<Option<LoginInput>> {
    let script = r#"
set emailPrompt to display dialog "BePure login" default answer "" buttons {"Cancel", "Next"} default button "Next"
set emailValue to text returned of emailPrompt
set passwordPrompt to display dialog "Password" default answer "" with hidden answer buttons {"Cancel", "Sign in"} default button "Sign in"
set passwordValue to text returned of passwordPrompt
return emailValue & "__BEPURE_SPLIT__" & passwordValue
"#;

    let Some(raw) = run_script_allow_cancel(script)? else {
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

pub fn prompt_logged_in_action(
    email: &str,
    device_id: &str,
    screenshot_permission: &str,
    daemon_status_updated_at: Option<&str>,
    daemon_last_error: Option<&str>,
) -> Result<Option<LoggedInAction>> {
    let message = format!(
        "Signed in as {email}.\nDevice id: {device_id}\n\nDaemon status:\nScreen Recording permission: {screenshot_permission}\nLast status update: {}\nLast daemon error: {}",
        daemon_status_updated_at.unwrap_or("<none>"),
        daemon_last_error
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
    email: &str,
    device_id: &str,
    screenshot_permission: &str,
    daemon_status_updated_at: Option<&str>,
    last_error: Option<&str>,
) -> Result<Option<LoggedInAction>> {
    let mut message = format!(
        "Signed in as {email}.\nDevice id: {device_id}\n\nDaemon status:\nScreen Recording permission: {screenshot_permission}\nLast status update: {}\nLast daemon error: {}",
        daemon_status_updated_at.unwrap_or("<none>"),
        last_error
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("<none>")
    );
    message.push_str(
        "\n\nScreen Recording permission appears to be missing for the BePure background service.\n\nOpen System Settings > Privacy & Security > Screen Recording, enable BePure, then click Restart daemon. Restart is required even if you selected Quit & Reopen earlier.",
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
