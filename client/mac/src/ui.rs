use std::process::{Command, Output};

use anyhow::{Result, anyhow};

const LOGIN_SPLIT: &str = "__BEPURE_SPLIT__";

#[derive(Debug, Clone)]
pub struct LoginInput {
    pub email: String,
    pub password: String,
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

pub fn prompt_logged_in_action(device_id: &str) -> Result<Option<bool>> {
    let escaped_id = apple_script_escape(device_id);
    let script = format!(
        r#"
set dialogResult to display dialog "Signed in.\nDevice id: {escaped_id}" buttons {{"Close", "Logout"}} default button "Close"
return button returned of dialogResult
"#
    );

    let Some(raw) = run_script_allow_cancel(&script)? else {
        return Ok(None);
    };

    match raw.trim() {
        "Logout" => Ok(Some(true)),
        "Close" => Ok(Some(false)),
        _ => Ok(None),
    }
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
