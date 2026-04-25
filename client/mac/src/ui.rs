use std::cell::OnceCell;
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSApplication, NSBackingStoreType,
    NSBorderType, NSButton, NSColor, NSEventModifierFlags, NSImage, NSImageView, NSMenu,
    NSMenuItem, NSModalResponse, NSScrollView, NSSecureTextField, NSTextField, NSTextView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use tao::event_loop::EventLoopProxy;

const LOGIN_RESPONSE_SIGN_IN: NSModalResponse = 1;
const LOGIN_RESPONSE_CANCEL: NSModalResponse = 0;
const ACTION_RESPONSE_CLOSE: NSModalResponse = 1;
const VIRTUE_WEBSITE_URL: &str = "https://virtueinitiative.org";
static MAIN_WINDOW_EVENT_PROXY: OnceLock<EventLoopProxy<MainWindowEvent>> = OnceLock::new();

pub fn install_main_window_event_proxy(proxy: EventLoopProxy<MainWindowEvent>) {
    let _ = MAIN_WINDOW_EVENT_PROXY.set(proxy);
}

#[derive(Debug, Clone)]
pub struct LoginInput {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoggedInAction {
    StopMonitoring,
    Logout,
    AllowScreenCapture,
    RelaunchToAcceptPermissions,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainWindowEvent {
    Action(LoggedInAction),
    Closed,
}

pub struct LoggedInDialogDetails<'a> {
    pub build_label: &'a str,
    pub email: &'a str,
    pub show_permission_actions: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionSectionState {
    pub show_permission_actions: bool,
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

        #[unsafe(method(openWebsite:))]
        fn open_website(&self, _sender: Option<&AnyObject>) {
            let _ = open_virtue_website();
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

#[derive(Debug, Default)]
struct MainWindowIvars {
    window: OnceCell<Retained<NSWindow>>,
    event_proxy: OnceCell<EventLoopProxy<MainWindowEvent>>,
    permission_label: OnceCell<Retained<NSTextField>>,
    allow_button: OnceCell<Retained<NSButton>>,
    relaunch_button: OnceCell<Retained<NSButton>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = MainWindowIvars]
    struct MainWindowController;

    impl MainWindowController {
        #[unsafe(method(stopMonitoring:))]
        fn stop_monitoring(&self, _sender: Option<&AnyObject>) {
            let _ = self.emit(MainWindowEvent::Action(LoggedInAction::StopMonitoring));
        }

        #[unsafe(method(logout:))]
        fn logout(&self, _sender: Option<&AnyObject>) {
            let _ = self.emit(MainWindowEvent::Action(LoggedInAction::Logout));
        }

        #[unsafe(method(allowScreenCapture:))]
        fn allow_screen_capture(&self, _sender: Option<&AnyObject>) {
            let _ = self.emit(MainWindowEvent::Action(LoggedInAction::AllowScreenCapture));
        }

        #[unsafe(method(relaunchToAcceptPermissions:))]
        fn relaunch_to_accept_permissions(&self, _sender: Option<&AnyObject>) {
            let _ = self.emit(MainWindowEvent::Action(LoggedInAction::RelaunchToAcceptPermissions));
        }

        #[unsafe(method(showStatus:))]
        fn show_status(&self, _sender: Option<&AnyObject>) {
            let _ = self.emit(MainWindowEvent::Action(LoggedInAction::Status));
        }

        #[unsafe(method(openWebsite:))]
        fn open_website(&self, _sender: Option<&AnyObject>) {
            let _ = open_virtue_website();
        }
    }

    unsafe impl NSObjectProtocol for MainWindowController {}

    unsafe impl NSWindowDelegate for MainWindowController {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            let _ = self.emit(MainWindowEvent::Closed);
        }
    }
);

impl MainWindowController {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(MainWindowIvars::default());
        unsafe { msg_send![super(this), init] }
    }

    fn set_window_and_proxy(
        &self,
        window: Retained<NSWindow>,
        event_proxy: EventLoopProxy<MainWindowEvent>,
        permission_label: Retained<NSTextField>,
        allow_button: Retained<NSButton>,
        relaunch_button: Retained<NSButton>,
    ) {
        self.ivars().window.set(window).expect("window already set");
        self.ivars()
            .event_proxy
            .set(event_proxy)
            .expect("event proxy already set");
        self.ivars()
            .permission_label
            .set(permission_label)
            .expect("permission label already set");
        self.ivars()
            .allow_button
            .set(allow_button)
            .expect("allow button already set");
        self.ivars()
            .relaunch_button
            .set(relaunch_button)
            .expect("relaunch button already set");
    }

    fn update_permission_section(&self, state: PermissionSectionState) {
        self.ivars()
            .permission_label
            .get()
            .expect("permission label must be set")
            .setHidden(!state.show_permission_actions);
        self.ivars()
            .allow_button
            .get()
            .expect("allow button must be set")
            .setHidden(!state.show_permission_actions);
        self.ivars()
            .relaunch_button
            .get()
            .expect("relaunch button must be set")
            .setHidden(!state.show_permission_actions);
    }

    fn emit(&self, event: MainWindowEvent) -> Result<()> {
        let proxy = self
            .ivars()
            .event_proxy
            .get()
            .context("main window event proxy must be set")?;
        proxy
            .send_event(event)
            .map_err(|_| anyhow!("failed to send main window event"))?;
        Ok(())
    }
}

pub struct MainWindowHandle {
    controller: Retained<MainWindowController>,
    window: Retained<NSWindow>,
}

impl MainWindowHandle {
    pub fn focus(&self) -> Result<()> {
        let mtm = appkit_thread_marker()?;
        let app = NSApplication::sharedApplication(mtm);
        install_standard_menus(mtm);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        self.window.makeKeyAndOrderFront(None);
        Ok(())
    }

    pub fn close(&self) {
        self.window.close();
    }

    pub fn update_permission_section(&self, state: PermissionSectionState) {
        self.controller.update_permission_section(state);
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
    let title = format!("Virtue login - virtueinitiative.org ({build_label})");
    show_login_window(&title, default_email.unwrap_or_default(), attempt_login)
}

pub fn show_main_window(details: &LoggedInDialogDetails<'_>) -> Result<MainWindowHandle> {
    let mtm = appkit_thread_marker()?;
    let controller = MainWindowController::new(mtm);
    let window_width = 700.0;
    let window_height = 290.0;
    let rail_width = 160.0;
    let content_x = rail_width + 24.0;
    let content_width = window_width - content_x - 20.0;
    let window = build_main_window(
        mtm,
        "Virtue - virtueinitiative.org",
        window_width,
        window_height,
    )?;
    window.setBackgroundColor(Some(&NSColor::controlBackgroundColor()));
    let content = window
        .contentView()
        .context("window must have content view")?;
    let content_background = visual_effect_view(
        mtm,
        NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(window_width, window_height),
        ),
        NSVisualEffectMaterial::ContentBackground,
        NSVisualEffectBlendingMode::WithinWindow,
    );
    let sidebar_background = visual_effect_view(
        mtm,
        NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(rail_width, window_height),
        ),
        NSVisualEffectMaterial::Sidebar,
        NSVisualEffectBlendingMode::WithinWindow,
    );
    let logo_view = build_logo_view(
        mtm,
        NSRect::new(NSPoint::new(28.0, 96.0), NSSize::new(104.0, 104.0)),
    )?;

    let message_label = wrapping_label(
        mtm,
        &format_main_dialog_message(details),
        content_x,
        156.0,
        content_width,
        74.0,
    );
    let permission_label = wrapping_label(
        mtm,
        "The Virtue service needs screen permissions. Allow screen capture and then click Relaunch to Accept Permissions.",
        content_x,
        108.0,
        content_width,
        36.0,
    );
    let status_button = button(
        mtm,
        "Status",
        NSRect::new(NSPoint::new(content_x, 20.0), NSSize::new(92.0, 28.0)),
        &controller,
        sel!(showStatus:),
        None,
    );
    let allow_screen_capture_button = button(
        mtm,
        "Allow Screen Capture",
        NSRect::new(NSPoint::new(content_x, 64.0), NSSize::new(156.0, 28.0)),
        &controller,
        sel!(allowScreenCapture:),
        None,
    );
    let relaunch_button = button(
        mtm,
        "Relaunch to Accept Permissions",
        NSRect::new(
            NSPoint::new(content_x + 164.0, 64.0),
            NSSize::new(220.0, 28.0),
        ),
        &controller,
        sel!(relaunchToAcceptPermissions:),
        None,
    );
    let website_button = button(
        mtm,
        "Open Website",
        NSRect::new(
            NSPoint::new(content_x + 100.0, 20.0),
            NSSize::new(120.0, 28.0),
        ),
        &controller,
        sel!(openWebsite:),
        None,
    );
    let stop_button = button(
        mtm,
        "Stop Monitoring",
        NSRect::new(
            NSPoint::new(content_x + 228.0, 20.0),
            NSSize::new(136.0, 28.0),
        ),
        &controller,
        sel!(stopMonitoring:),
        None,
    );
    let logout_button = button(
        mtm,
        "Logout",
        NSRect::new(
            NSPoint::new(content_x + 372.0, 20.0),
            NSSize::new(88.0, 28.0),
        ),
        &controller,
        sel!(logout:),
        None,
    );

    content.addSubview(&content_background);
    content.addSubview(&sidebar_background);
    content.addSubview(&logo_view);
    content.addSubview(&message_label);
    content.addSubview(&permission_label);
    content.addSubview(&status_button);
    content.addSubview(&allow_screen_capture_button);
    content.addSubview(&relaunch_button);
    content.addSubview(&website_button);
    content.addSubview(&stop_button);
    content.addSubview(&logout_button);

    let proxy = MAIN_WINDOW_EVENT_PROXY
        .get()
        .cloned()
        .context("main window event proxy not initialized")?;
    controller.set_window_and_proxy(
        window.clone(),
        proxy,
        permission_label.clone(),
        allow_screen_capture_button.clone(),
        relaunch_button.clone(),
    );
    controller.update_permission_section(PermissionSectionState {
        show_permission_actions: details.show_permission_actions,
    });
    window.setDelegate(Some(ProtocolObject::from_ref(&*controller)));
    show_modeless_window(mtm, &window);

    Ok(MainWindowHandle { controller, window })
}

pub fn confirm_stop_monitoring() -> Result<bool> {
    confirm_action(
        "Virtue stop monitoring",
        "Stopping the background service will alert people monitoring you. Continue?",
        "Stop Monitoring",
    )
}

pub fn confirm_logout() -> Result<bool> {
    confirm_action(
        "Virtue logout",
        "Logging out will alert people monitoring you and will recreate a new device on your next login. Continue?",
        "Logout",
    )
}

pub fn show_info(message: &str) -> Result<()> {
    show_alert("Virtue", message, NSAlertStyle::Informational)?;
    Ok(())
}

pub fn show_warning(message: &str) -> Result<()> {
    show_alert("Virtue", message, NSAlertStyle::Warning)?;
    Ok(())
}

pub fn show_error(message: &str) -> Result<()> {
    show_alert("Operation failed", message, NSAlertStyle::Critical)?;
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
        NSRect::new(NSPoint::new(174.0, 14.0), NSSize::new(90.0, 28.0)),
        &controller,
        sel!(submit:),
        Some("\r"),
    );
    let website_button = button(
        mtm,
        "Open Website",
        NSRect::new(NSPoint::new(270.0, 14.0), NSSize::new(90.0, 28.0)),
        &controller,
        sel!(openWebsite:),
        None,
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
    content.addSubview(&website_button);
    content.addSubview(&cancel_button);

    unsafe {
        email_field.setNextKeyView(Some(&password_field));
        password_field.setNextKeyView(Some(&sign_in_button));
        sign_in_button.setNextKeyView(Some(&website_button));
        website_button.setNextKeyView(Some(&cancel_button));
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

fn build_main_window(
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
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
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
    install_standard_menus(app.mtm());
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    window.makeKeyAndOrderFront(None);

    if let Some(initial_responder) = initial_responder {
        let _ = window.makeFirstResponder(Some(initial_responder));
        unsafe { initial_responder.selectText(None) };
    }

    app.runModalForWindow(window)
}

fn show_modeless_window(mtm: MainThreadMarker, window: &NSWindow) {
    let app = NSApplication::sharedApplication(mtm);
    install_standard_menus(app.mtm());
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    window.makeKeyAndOrderFront(None);
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

pub fn show_status(message: &str) -> Result<()> {
    show_text_window("Virtue status", message, 560.0, 440.0)
}

fn format_main_dialog_message(details: &LoggedInDialogDetails<'_>) -> String {
    format!(
        "Version: {}\nSigned in as {}",
        details.build_label, details.email
    )
}

fn build_logo_view(mtm: MainThreadMarker, frame: NSRect) -> Result<Retained<NSImageView>> {
    let image_path =
        NSString::from_str(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/AppIcon.icns"));
    let image = NSImage::initWithContentsOfFile(NSImage::alloc(), &image_path)
        .context("failed to load app icon asset")?;
    image.setSize(NSSize::new(frame.size.width, frame.size.height));

    let image_view = NSImageView::initWithFrame(NSImageView::alloc(mtm), frame);
    image_view.setImage(Some(&image));
    Ok(image_view)
}

fn visual_effect_view(
    mtm: MainThreadMarker,
    frame: NSRect,
    material: NSVisualEffectMaterial,
    blending_mode: NSVisualEffectBlendingMode,
) -> Retained<NSVisualEffectView> {
    let view = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), frame);
    view.setMaterial(material);
    view.setBlendingMode(blending_mode);
    view.setState(NSVisualEffectState::FollowsWindowActiveState);
    view
}

fn open_virtue_website() -> Result<()> {
    let status = Command::new("open")
        .arg(VIRTUE_WEBSITE_URL)
        .status()
        .context("failed to launch website opener")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("website opener exited with status {status}"))
    }
}

fn appkit_thread_marker() -> Result<MainThreadMarker> {
    MainThreadMarker::new().context("AppKit UI must run on the main thread")
}

fn confirm_action(title: &str, message: &str, continue_label: &str) -> Result<bool> {
    let mtm = appkit_thread_marker()?;
    let app = NSApplication::sharedApplication(mtm);
    install_standard_menus(mtm);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);

    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(message));
    alert.addButtonWithTitle(&NSString::from_str(continue_label));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));

    Ok(alert.runModal() == NSAlertFirstButtonReturn)
}

fn show_alert(title: &str, message: &str, style: NSAlertStyle) -> Result<()> {
    let mtm = appkit_thread_marker()?;
    let app = NSApplication::sharedApplication(mtm);
    install_standard_menus(mtm);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);

    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(style);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(message));
    alert.addButtonWithTitle(&NSString::from_str("OK"));
    let _ = alert.runModal();
    Ok(())
}

fn show_text_window(title: &str, message: &str, width: f64, height: f64) -> Result<()> {
    let mtm = appkit_thread_marker()?;
    let controller = ActionWindowController::new(mtm);
    let window = build_window(mtm, title, width, height)?;
    let content = window
        .contentView()
        .context("window must have content view")?;

    let scroll_view = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(20.0, 60.0),
            NSSize::new(width - 40.0, height - 90.0),
        ),
    );
    scroll_view.setBorderType(NSBorderType::BezelBorder);
    scroll_view.setHasVerticalScroller(true);
    scroll_view.setAutohidesScrollers(true);

    let text_view = NSTextView::initWithFrame(
        NSTextView::alloc(mtm),
        NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(width - 40.0, height - 90.0),
        ),
    );
    text_view.setString(&NSString::from_str(message));
    text_view.setEditable(false);
    text_view.setSelectable(true);
    text_view.setRichText(false);
    text_view.setTextContainerInset(NSSize::new(8.0, 8.0));
    scroll_view.setDocumentView(Some(&text_view));

    let close_button = button(
        mtm,
        "Close",
        NSRect::new(NSPoint::new(width - 100.0, 18.0), NSSize::new(80.0, 28.0)),
        &controller,
        sel!(closeWindow:),
        Some("\r"),
    );

    content.addSubview(&scroll_view);
    content.addSubview(&close_button);

    controller.set_window(window.clone());
    let _ = run_modal_window(mtm, &window, None);
    window.orderOut(None);
    Ok(())
}

fn install_standard_menus(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    if app.mainMenu().is_some() {
        return;
    }

    let main_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Main Menu"));

    let app_menu_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Virtue"),
            None,
            &NSString::from_str(""),
        )
    };
    let app_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Virtue"));
    let quit_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Quit Virtue"),
            Some(sel!(terminate:)),
            &NSString::from_str("q"),
        )
    };
    quit_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    app_menu.addItem(&quit_item);
    app_menu_item.setSubmenu(Some(&app_menu));
    main_menu.addItem(&app_menu_item);

    let edit_menu_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Edit"),
            None,
            &NSString::from_str(""),
        )
    };
    let edit_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Edit"));

    add_edit_menu_item(mtm, &edit_menu, "Undo", Some(sel!(undo:)), "z", false);
    add_edit_menu_item(mtm, &edit_menu, "Redo", Some(sel!(redo:)), "Z", true);
    edit_menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_edit_menu_item(mtm, &edit_menu, "Cut", Some(sel!(cut:)), "x", false);
    add_edit_menu_item(mtm, &edit_menu, "Copy", Some(sel!(copy:)), "c", false);
    add_edit_menu_item(mtm, &edit_menu, "Paste", Some(sel!(paste:)), "v", false);
    add_edit_menu_item(
        mtm,
        &edit_menu,
        "Select All",
        Some(sel!(selectAll:)),
        "a",
        false,
    );

    edit_menu_item.setSubmenu(Some(&edit_menu));
    main_menu.addItem(&edit_menu_item);

    app.setMainMenu(Some(&main_menu));
}

fn add_edit_menu_item(
    mtm: MainThreadMarker,
    menu: &NSMenu,
    title: &str,
    action: Option<objc2::runtime::Sel>,
    key: &str,
    include_shift: bool,
) {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key),
        )
    };
    let mut modifiers = NSEventModifierFlags::Command;
    if include_shift {
        modifiers |= NSEventModifierFlags::Shift;
    }
    item.setKeyEquivalentModifierMask(modifiers);
    menu.addItem(&item);
}
