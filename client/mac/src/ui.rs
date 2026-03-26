use std::cell::OnceCell;
use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSButton, NSModalResponse, NSSecureTextField, NSTextField,
    NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

const LOGIN_RESPONSE_SIGN_IN: NSModalResponse = 1;
const LOGIN_RESPONSE_CANCEL: NSModalResponse = 0;
const ACTION_RESPONSE_CLOSE: NSModalResponse = 1;
const ACTION_RESPONSE_RESTART: NSModalResponse = 2;
const ACTION_RESPONSE_LOGOUT: NSModalResponse = 3;

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

#[derive(Debug, Default)]
struct LoginWindowIvars {
    window: OnceCell<Retained<NSWindow>>,
    email_field: OnceCell<Retained<NSTextField>>,
    password_field: OnceCell<Retained<NSSecureTextField>>,
    error_label: OnceCell<Retained<NSTextField>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = LoginWindowIvars]
    struct LoginWindowController;

    impl LoginWindowController {
        #[unsafe(method(submit:))]
        fn submit(&self, _sender: Option<&AnyObject>) {
            self.finish_modal(LOGIN_RESPONSE_SIGN_IN);
        }

        #[unsafe(method(cancel:))]
        fn cancel(&self, _sender: Option<&AnyObject>) {
            self.finish_modal(LOGIN_RESPONSE_CANCEL);
        }
    }

    unsafe impl NSObjectProtocol for LoginWindowController {}
);

impl LoginWindowController {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(LoginWindowIvars::default());
        unsafe { msg_send![super(this), init] }
    }

    fn set_controls(
        &self,
        window: Retained<NSWindow>,
        email_field: Retained<NSTextField>,
        password_field: Retained<NSSecureTextField>,
        error_label: Retained<NSTextField>,
    ) {
        self.ivars().window.set(window).expect("window already set");
        self.ivars()
            .email_field
            .set(email_field)
            .expect("email field already set");
        self.ivars()
            .password_field
            .set(password_field)
            .expect("password field already set");
        self.ivars()
            .error_label
            .set(error_label)
            .expect("error label already set");
    }

    fn finish_modal(&self, response: NSModalResponse) {
        let app = NSApplication::sharedApplication(self.mtm());
        app.stopModalWithCode(response);
    }

    fn email_value(&self) -> String {
        self.ivars()
            .email_field
            .get()
            .expect("email field must be set")
            .stringValue()
            .to_string()
    }

    fn password_value(&self) -> String {
        self.ivars()
            .password_field
            .get()
            .expect("password field must be set")
            .stringValue()
            .to_string()
    }

    fn set_password_value(&self, value: &str) {
        self.ivars()
            .password_field
            .get()
            .expect("password field must be set")
            .setStringValue(&NSString::from_str(value));
    }

    fn set_error_message(&self, message: &str) {
        self.ivars()
            .error_label
            .get()
            .expect("error label must be set")
            .setStringValue(&NSString::from_str(message));
    }
}

#[derive(Debug, Default)]
struct ActionWindowIvars {
    window: OnceCell<Retained<NSWindow>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ActionWindowIvars]
    struct ActionWindowController;

    impl ActionWindowController {
        #[unsafe(method(closeWindow:))]
        fn close_window(&self, _sender: Option<&AnyObject>) {
            self.finish_modal(ACTION_RESPONSE_CLOSE);
        }

        #[unsafe(method(restartDaemon:))]
        fn restart_daemon(&self, _sender: Option<&AnyObject>) {
            self.finish_modal(ACTION_RESPONSE_RESTART);
        }

        #[unsafe(method(logout:))]
        fn logout(&self, _sender: Option<&AnyObject>) {
            self.finish_modal(ACTION_RESPONSE_LOGOUT);
        }
    }

    unsafe impl NSObjectProtocol for ActionWindowController {}
);

impl ActionWindowController {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ActionWindowIvars::default());
        unsafe { msg_send![super(this), init] }
    }

    fn set_window(&self, window: Retained<NSWindow>) {
        self.ivars().window.set(window).expect("window already set");
    }

    fn finish_modal(&self, response: NSModalResponse) {
        let app = NSApplication::sharedApplication(self.mtm());
        let window = self.ivars().window.get().expect("window must be set");
        app.stopModalWithCode(response);
        window.orderOut(None);
    }
}

pub fn prompt_login<F>(
    build_label: &str,
    default_email: Option<&str>,
    attempt_login: F,
) -> Result<Option<String>>
where
    F: FnMut(&LoginInput) -> std::result::Result<String, String>,
{
    let title = format!("Virtue login ({build_label})");
    show_login_window(&title, default_email.unwrap_or_default(), attempt_login)
}

pub fn prompt_logged_in_action(
    details: &LoggedInDialogDetails<'_>,
) -> Result<Option<LoggedInAction>> {
    show_action_window("Virtue", &format_status_message(details), false)
}

pub fn prompt_permission_issue_action(
    details: &LoggedInDialogDetails<'_>,
) -> Result<Option<LoggedInAction>> {
    let mut message = format_status_message(details);
    message.push_str(
        "\n\nScreen Recording permission appears to be missing for the Virtue background service.\n\nOpen System Settings > Privacy & Security > Screen Recording, enable Virtue, then click Restart daemon. Restart is required even if you selected Quit & Reopen earlier.",
    );
    show_action_window("Virtue permission required", &message, true)
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

fn show_login_window<F>(
    title: &str,
    default_email: &str,
    mut attempt_login: F,
) -> Result<Option<String>>
where
    F: FnMut(&LoginInput) -> std::result::Result<String, String>,
{
    let mtm = appkit_thread_marker()?;
    let controller = LoginWindowController::new(mtm);

    let window = build_window(mtm, title, 460.0, 248.0)?;
    let content = window
        .contentView()
        .context("window must have content view")?;

    let header = wrapping_label(
        mtm,
        "Enter your Virtue account credentials to sign in on this device.",
        20.0,
        188.0,
        420.0,
        34.0,
    );
    let error_label = wrapping_label(mtm, "", 20.0, 154.0, 420.0, 24.0);
    let email_label = label(mtm, "Email", 20.0, 128.0, 120.0, 20.0);
    let email_field = text_input(
        mtm,
        default_email,
        Some("name@example.com"),
        20.0,
        102.0,
        420.0,
        24.0,
    );
    let password_label = label(mtm, "Password", 20.0, 76.0, 120.0, 20.0);
    let password_field = secure_input(mtm, Some("Password"), 20.0, 50.0, 420.0, 24.0);

    let sign_in_button = button(
        mtm,
        "Sign in",
        NSRect::new(NSPoint::new(270.0, 14.0), NSSize::new(90.0, 28.0)),
        &controller,
        sel!(submit:),
        Some("\r"),
    );
    let cancel_button = button(
        mtm,
        "Cancel",
        NSRect::new(NSPoint::new(366.0, 14.0), NSSize::new(74.0, 28.0)),
        &controller,
        sel!(cancel:),
        None,
    );

    content.addSubview(&header);
    content.addSubview(&error_label);
    content.addSubview(&email_label);
    content.addSubview(&email_field);
    content.addSubview(&password_label);
    content.addSubview(&password_field);
    content.addSubview(&sign_in_button);
    content.addSubview(&cancel_button);

    unsafe {
        email_field.setNextKeyView(Some(&password_field));
        password_field.setNextKeyView(Some(&sign_in_button));
        sign_in_button.setNextKeyView(Some(&cancel_button));
        cancel_button.setNextKeyView(Some(&email_field));
    }

    window.setInitialFirstResponder(Some(&email_field));
    controller.set_controls(
        window.clone(),
        email_field.clone(),
        password_field.clone(),
        error_label.clone(),
    );

    loop {
        let response = run_modal_window(mtm, &window, Some(&email_field));
        if response != LOGIN_RESPONSE_SIGN_IN {
            window.orderOut(None);
            return Ok(None);
        }

        let input = LoginInput {
            email: controller.email_value().trim().to_string(),
            password: controller.password_value(),
        };

        if input.email.is_empty() || input.password.is_empty() {
            controller.set_error_message("Email and password are required.");
            continue;
        }

        match attempt_login(&input) {
            Ok(device_id) => {
                window.orderOut(None);
                return Ok(Some(device_id));
            }
            Err(message) => {
                controller.set_password_value("");
                controller.set_error_message(&message);
            }
        }
    }
}

fn show_action_window(
    title: &str,
    message: &str,
    emphasize_restart: bool,
) -> Result<Option<LoggedInAction>> {
    let mtm = appkit_thread_marker()?;
    let controller = ActionWindowController::new(mtm);

    let window_height = if emphasize_restart { 450.0 } else { 360.0 };
    let message_height = if emphasize_restart { 350.0 } else { 260.0 };

    let window = build_window(mtm, title, 620.0, window_height)?;
    let content = window
        .contentView()
        .context("window must have content view")?;

    let message_label = wrapping_label(mtm, message, 20.0, 74.0, 580.0, message_height);
    let close_button = button(
        mtm,
        "Close",
        NSRect::new(NSPoint::new(224.0, 18.0), NSSize::new(120.0, 28.0)),
        &controller,
        sel!(closeWindow:),
        if emphasize_restart { None } else { Some("\r") },
    );
    let restart_button = button(
        mtm,
        "Restart daemon",
        NSRect::new(NSPoint::new(352.0, 18.0), NSSize::new(120.0, 28.0)),
        &controller,
        sel!(restartDaemon:),
        if emphasize_restart { Some("\r") } else { None },
    );
    let logout_button = button(
        mtm,
        "Logout",
        NSRect::new(NSPoint::new(480.0, 18.0), NSSize::new(120.0, 28.0)),
        &controller,
        sel!(logout:),
        None,
    );

    content.addSubview(&message_label);
    content.addSubview(&close_button);
    content.addSubview(&restart_button);
    content.addSubview(&logout_button);

    controller.set_window(window.clone());
    let response = run_modal_window(mtm, &window, None);
    window.orderOut(None);
    Ok(parse_logged_in_action(response))
}

fn build_window(
    mtm: MainThreadMarker,
    title: &str,
    width: f64,
    height: f64,
) -> Result<Retained<NSWindow>> {
    let title = NSString::from_str(title);
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height)),
            NSWindowStyleMask::Titled,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&title);
    window.center();
    Ok(window)
}

fn run_modal_window(
    mtm: MainThreadMarker,
    window: &NSWindow,
    initial_responder: Option<&NSTextField>,
) -> NSModalResponse {
    let app = NSApplication::sharedApplication(mtm);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    window.makeKeyAndOrderFront(None);

    if let Some(initial_responder) = initial_responder {
        let _ = window.makeFirstResponder(Some(initial_responder));
        unsafe { initial_responder.selectText(None) };
    }

    app.runModalForWindow(window)
}

fn label(
    mtm: MainThreadMarker,
    text: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)));
    label
}

fn wrapping_label(
    mtm: MainThreadMarker,
    text: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Retained<NSTextField> {
    let label = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
    label.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)));
    label
}

fn text_input(
    mtm: MainThreadMarker,
    value: &str,
    placeholder: Option<&str>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Retained<NSTextField> {
    let field = NSTextField::initWithFrame(
        NSTextField::alloc(mtm),
        NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)),
    );
    if let Some(placeholder) = placeholder {
        let placeholder = NSString::from_str(placeholder);
        field.setPlaceholderString(Some(&placeholder));
    }
    field.setStringValue(&NSString::from_str(value));
    field
}

fn secure_input(
    mtm: MainThreadMarker,
    placeholder: Option<&str>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Retained<NSSecureTextField> {
    let field = NSSecureTextField::initWithFrame(
        NSSecureTextField::alloc(mtm),
        NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)),
    );
    if let Some(placeholder) = placeholder {
        let placeholder = NSString::from_str(placeholder);
        field.setPlaceholderString(Some(&placeholder));
    }
    field
}

fn button(
    mtm: MainThreadMarker,
    title: &str,
    frame: NSRect,
    target: &AnyObject,
    action: objc2::runtime::Sel,
    key_equivalent: Option<&str>,
) -> Retained<NSButton> {
    let button = NSButton::initWithFrame(NSButton::alloc(mtm), frame);
    button.setTitle(&NSString::from_str(title));
    unsafe {
        button.setTarget(Some(target));
        button.setAction(Some(action));
    }
    if let Some(key_equivalent) = key_equivalent {
        button.setKeyEquivalent(&NSString::from_str(key_equivalent));
    }
    button
}

fn format_status_message(details: &LoggedInDialogDetails<'_>) -> String {
    format!(
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
    )
}

fn appkit_thread_marker() -> Result<MainThreadMarker> {
    MainThreadMarker::new().context("AppKit UI must run on the main thread")
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

fn parse_logged_in_action(response: NSModalResponse) -> Option<LoggedInAction> {
    match response {
        ACTION_RESPONSE_CLOSE => Some(LoggedInAction::Close),
        ACTION_RESPONSE_RESTART => Some(LoggedInAction::RestartDaemon),
        ACTION_RESPONSE_LOGOUT => Some(LoggedInAction::Logout),
        _ => None,
    }
}
