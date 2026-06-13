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
    NSMenuItem, NSModalResponse, NSScrollView, NSSecureTextField, NSTextField, NSTextView, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use tao::event_loop::EventLoopProxy;

const ACTION_RESPONSE_CLOSE: NSModalResponse = 1;
const VIRTUE_WEBSITE_URL: &str = "https://virtueinitiative.org";
const VIRTUE_SIGNUP_URL: &str = "https://app.virtueinitiative.org/signup";
static MAIN_WINDOW_EVENT_PROXY: OnceLock<EventLoopProxy<MainWindowEvent>> = OnceLock::new();

pub fn install_main_window_event_proxy(proxy: EventLoopProxy<MainWindowEvent>) {
    let _ = MAIN_WINDOW_EVENT_PROXY.set(proxy);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionPhase {
    NeedsRequest,
    NeedsRelaunch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoggedInAction {
    StopMonitoring,
    Logout,
    RequestPermissions,
    RelaunchToAcceptPermissions,
    Status,
}

/// Result of a single daemon status poll.
///
/// The daemon's lifecycle module always reports `is_running: true` when it can
/// answer a `StatusRequest`, so the meaningful distinction is *how* the poll
/// failed: a refused connection means the daemon is genuinely gone, while a
/// timeout means it is alive but busy (e.g. blocked on the login network call).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonStatus {
    /// Connected and got a status response — daemon is running.
    Running,
    /// Connection refused — nothing is listening, daemon is not running.
    Stopped,
    /// Connected but no timely response (or other transient IPC error). The
    /// daemon is alive but busy; do not treat this as "stopped".
    Unreachable,
}

#[derive(Debug, Clone)]
pub enum MainWindowEvent {
    Action(LoggedInAction),
    Closed,
    LoginSubmitted {
        email: String,
        password: String,
    },
    /// Sent from background thread when relaunch completes. `None` = success.
    RelaunchDone(Option<String>),
    /// Sent from background poll thread so the main thread never blocks on IPC.
    StatusPolled {
        logged_in: bool,
        status: DaemonStatus,
        has_capture_permission: bool,
    },
}

pub fn send_main_window_event(event: MainWindowEvent) -> Result<()> {
    MAIN_WINDOW_EVENT_PROXY
        .get()
        .context("main window event proxy not initialized")?
        .send_event(event)
        .map_err(|_| anyhow!("event loop closed"))?;
    Ok(())
}

pub struct MainWindowDetails<'a> {
    pub build_label: &'a str,
    pub mode: MainWindowMode<'a>,
}

pub enum MainWindowMode<'a> {
    Login {
        default_email: &'a str,
    },
    LoggedIn {
        email: &'a str,
        phase: Option<PermissionPhase>,
    },
}

// -- ActionWindowController (for status text window) ---------------------------

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

// -- MainWindowController ------------------------------------------------------

#[derive(Debug, Default)]
struct MainWindowIvars {
    window: OnceCell<Retained<NSWindow>>,
    event_proxy: OnceCell<EventLoopProxy<MainWindowEvent>>,
    // Login mode
    login_container: OnceCell<Retained<NSView>>,
    login_email_field: OnceCell<Retained<NSTextField>>,
    login_password_field: OnceCell<Retained<NSSecureTextField>>,
    login_error_label: OnceCell<Retained<NSTextField>>,
    // Logged-in mode
    logged_in_container: OnceCell<Retained<NSView>>,
    message_label: OnceCell<Retained<NSTextField>>,
    permission_label: OnceCell<Retained<NSTextField>>,
    request_permissions_button: OnceCell<Retained<NSButton>>,
    relaunch_button: OnceCell<Retained<NSButton>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = MainWindowIvars]
    struct MainWindowController;

    impl MainWindowController {
        #[unsafe(method(loginSubmit:))]
        fn login_submit(&self, _sender: Option<&AnyObject>) {
            let email = self
                .ivars()
                .login_email_field
                .get()
                .expect("login_email_field must be set")
                .stringValue()
                .to_string();
            let password = self
                .ivars()
                .login_password_field
                .get()
                .expect("login_password_field must be set")
                .stringValue()
                .to_string();
            let email = email.trim().to_string();
            if email.is_empty() || password.is_empty() {
                self.set_login_error("Email and password are required.");
                return;
            }
            self.set_login_error("");
            let _ = self.emit(MainWindowEvent::LoginSubmitted { email, password });
        }

        #[unsafe(method(stopMonitoring:))]
        fn stop_monitoring(&self, _sender: Option<&AnyObject>) {
            let _ = self.emit(MainWindowEvent::Action(LoggedInAction::StopMonitoring));
        }

        #[unsafe(method(logout:))]
        fn logout(&self, _sender: Option<&AnyObject>) {
            let _ = self.emit(MainWindowEvent::Action(LoggedInAction::Logout));
        }

        #[unsafe(method(requestPermissions:))]
        fn request_permissions(&self, _sender: Option<&AnyObject>) {
            let _ = self.emit(MainWindowEvent::Action(LoggedInAction::RequestPermissions));
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

        #[unsafe(method(openSignup:))]
        fn open_signup(&self, _sender: Option<&AnyObject>) {
            let _ = open_url(VIRTUE_SIGNUP_URL);
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

    fn set_all(
        &self,
        window: Retained<NSWindow>,
        proxy: EventLoopProxy<MainWindowEvent>,
        login_container: Retained<NSView>,
        login_email_field: Retained<NSTextField>,
        login_password_field: Retained<NSSecureTextField>,
        login_error_label: Retained<NSTextField>,
        logged_in_container: Retained<NSView>,
        message_label: Retained<NSTextField>,
        permission_label: Retained<NSTextField>,
        request_permissions_button: Retained<NSButton>,
        relaunch_button: Retained<NSButton>,
    ) {
        self.ivars().window.set(window).expect("window already set");
        self.ivars()
            .event_proxy
            .set(proxy)
            .expect("event proxy already set");
        self.ivars()
            .login_container
            .set(login_container)
            .expect("login_container already set");
        self.ivars()
            .login_email_field
            .set(login_email_field)
            .expect("login_email_field already set");
        self.ivars()
            .login_password_field
            .set(login_password_field)
            .expect("login_password_field already set");
        self.ivars()
            .login_error_label
            .set(login_error_label)
            .expect("login_error_label already set");
        self.ivars()
            .logged_in_container
            .set(logged_in_container)
            .expect("logged_in_container already set");
        self.ivars()
            .message_label
            .set(message_label)
            .expect("message_label already set");
        self.ivars()
            .permission_label
            .set(permission_label)
            .expect("permission_label already set");
        self.ivars()
            .request_permissions_button
            .set(request_permissions_button)
            .expect("request_permissions_button already set");
        self.ivars()
            .relaunch_button
            .set(relaunch_button)
            .expect("relaunch_button already set");
    }

    fn set_login_error(&self, msg: &str) {
        self.ivars()
            .login_error_label
            .get()
            .expect("login_error_label must be set")
            .setStringValue(&NSString::from_str(msg));
    }

    fn switch_to_login_mode(&self, default_email: &str) {
        let ivars = self.ivars();
        ivars
            .login_error_label
            .get()
            .expect("login_error_label must be set")
            .setStringValue(&NSString::from_str(""));
        ivars
            .login_email_field
            .get()
            .expect("login_email_field must be set")
            .setStringValue(&NSString::from_str(default_email));
        ivars
            .login_password_field
            .get()
            .expect("login_password_field must be set")
            .setStringValue(&NSString::from_str(""));
        ivars
            .logged_in_container
            .get()
            .expect("logged_in_container must be set")
            .setHidden(true);
        ivars
            .login_container
            .get()
            .expect("login_container must be set")
            .setHidden(false);
    }

    fn switch_to_logged_in_mode(&self, message: &str, phase: Option<PermissionPhase>) {
        self.ivars()
            .message_label
            .get()
            .expect("message_label must be set")
            .setStringValue(&NSString::from_str(message));
        self.apply_permission_phase(phase);
        self.ivars()
            .login_container
            .get()
            .expect("login_container must be set")
            .setHidden(true);
        self.ivars()
            .logged_in_container
            .get()
            .expect("logged_in_container must be set")
            .setHidden(false);
    }

    fn apply_permission_phase(&self, phase: Option<PermissionPhase>) {
        let perm_label = self
            .ivars()
            .permission_label
            .get()
            .expect("permission_label must be set");
        let req_btn = self
            .ivars()
            .request_permissions_button
            .get()
            .expect("request_permissions_button must be set");
        let rel_btn = self
            .ivars()
            .relaunch_button
            .get()
            .expect("relaunch_button must be set");
        match phase {
            None => {
                perm_label.setHidden(true);
                req_btn.setHidden(true);
                rel_btn.setHidden(true);
            }
            Some(PermissionPhase::NeedsRequest) => {
                perm_label.setStringValue(&NSString::from_str(
                    "Screen capture permission is needed. Click below to request access.",
                ));
                perm_label.setHidden(false);
                req_btn.setHidden(false);
                rel_btn.setHidden(true);
            }
            Some(PermissionPhase::NeedsRelaunch) => {
                perm_label.setStringValue(&NSString::from_str(
                    "Permission requested. Relaunch the service to apply.",
                ));
                perm_label.setHidden(false);
                req_btn.setHidden(true);
                rel_btn.setHidden(false);
            }
        }
    }

    fn set_relaunch_button_state(&self, title: &str, enabled: bool) {
        let btn = self
            .ivars()
            .relaunch_button
            .get()
            .expect("relaunch_button must be set");
        btn.setTitle(&NSString::from_str(title));
        btn.setEnabled(enabled);
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

    pub fn show_login_error(&self, message: &str) {
        self.controller.set_login_error(message);
    }

    pub fn switch_to_logged_in(
        &self,
        email: &str,
        build_label: &str,
        phase: Option<PermissionPhase>,
    ) {
        let msg = format!("Version: {}\nSigned in as {}", build_label, email);
        self.controller.switch_to_logged_in_mode(&msg, phase);
    }

    pub fn update_permission_phase(&self, phase: Option<PermissionPhase>) {
        self.controller.apply_permission_phase(phase);
    }

    pub fn set_relaunch_button_state(&self, title: &str, enabled: bool) {
        self.controller.set_relaunch_button_state(title, enabled);
    }

    pub fn switch_to_login(&self, default_email: &str) {
        self.controller.switch_to_login_mode(default_email);
    }
}

pub fn show_main_window(details: &MainWindowDetails<'_>) -> Result<MainWindowHandle> {
    let mtm = appkit_thread_marker()?;
    let controller = MainWindowController::new(mtm);
    let window_width = 700.0_f64;
    let window_height = 290.0_f64;
    let rail_width = 160.0_f64;
    let content_x = rail_width + 24.0;
    let content_width = window_width - content_x - 20.0; // 496

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

    // -- Login container -------------------------------------------------------
    let login_container = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(
            NSPoint::new(content_x, 0.0),
            NSSize::new(window_width - content_x, window_height),
        ),
    );
    let login_header = wrapping_label(
        mtm,
        "Sign in to your Virtue account to start monitoring.",
        0.0,
        206.0,
        content_width,
        54.0,
    );
    let login_error_label = wrapping_label(mtm, "", 0.0, 174.0, content_width, 24.0);
    let email_label = label(mtm, "Email", 0.0, 148.0, 120.0, 20.0);
    let login_email_field = text_input(
        mtm,
        match &details.mode {
            MainWindowMode::Login { default_email } => default_email,
            _ => "",
        },
        Some("name@example.com"),
        0.0,
        122.0,
        content_width,
        24.0,
    );
    let password_label = label(mtm, "Password", 0.0, 88.0, 120.0, 20.0);
    let login_password_field = secure_input(mtm, Some("Password"), 0.0, 62.0, content_width, 24.0);
    let sign_in_button = button(
        mtm,
        "Sign In",
        NSRect::new(NSPoint::new(0.0, 20.0), NSSize::new(88.0, 28.0)),
        &controller,
        sel!(loginSubmit:),
        Some("\r"),
    );
    let login_website_button = button(
        mtm,
        "Create an Account",
        NSRect::new(NSPoint::new(96.0, 20.0), NSSize::new(136.0, 28.0)),
        &controller,
        sel!(openSignup:),
        None,
    );

    unsafe {
        login_email_field.setNextKeyView(Some(&login_password_field));
        login_password_field.setNextKeyView(Some(&sign_in_button));
        sign_in_button.setNextKeyView(Some(&login_website_button));
        login_website_button.setNextKeyView(Some(&login_email_field));
    }

    login_container.addSubview(&login_header);
    login_container.addSubview(&login_error_label);
    login_container.addSubview(&email_label);
    login_container.addSubview(&login_email_field);
    login_container.addSubview(&password_label);
    login_container.addSubview(&login_password_field);
    login_container.addSubview(&sign_in_button);
    login_container.addSubview(&login_website_button);

    // -- Logged-in container ---------------------------------------------------
    let logged_in_container = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(
            NSPoint::new(content_x, 0.0),
            NSSize::new(window_width - content_x, window_height),
        ),
    );
    let message_label = wrapping_label(mtm, "", 0.0, 156.0, content_width, 74.0);
    let permission_label = wrapping_label(mtm, "", 0.0, 108.0, content_width, 36.0);
    let status_button = button(
        mtm,
        "Status",
        NSRect::new(NSPoint::new(0.0, 20.0), NSSize::new(92.0, 28.0)),
        &controller,
        sel!(showStatus:),
        None,
    );
    let li_website_button = button(
        mtm,
        "Open Website",
        NSRect::new(NSPoint::new(100.0, 20.0), NSSize::new(120.0, 28.0)),
        &controller,
        sel!(openWebsite:),
        None,
    );
    let stop_button = button(
        mtm,
        "Stop Monitoring",
        NSRect::new(NSPoint::new(228.0, 20.0), NSSize::new(136.0, 28.0)),
        &controller,
        sel!(stopMonitoring:),
        None,
    );
    let logout_button = button(
        mtm,
        "Logout",
        NSRect::new(NSPoint::new(372.0, 20.0), NSSize::new(88.0, 28.0)),
        &controller,
        sel!(logout:),
        None,
    );
    // Two permission buttons at the same position — only one visible at a time.
    let request_permissions_button = button(
        mtm,
        "Request Permissions",
        NSRect::new(NSPoint::new(0.0, 64.0), NSSize::new(170.0, 28.0)),
        &controller,
        sel!(requestPermissions:),
        None,
    );
    let relaunch_button = button(
        mtm,
        "Relaunch to Accept Permissions",
        NSRect::new(NSPoint::new(0.0, 64.0), NSSize::new(240.0, 28.0)),
        &controller,
        sel!(relaunchToAcceptPermissions:),
        None,
    );

    logged_in_container.addSubview(&message_label);
    logged_in_container.addSubview(&permission_label);
    logged_in_container.addSubview(&status_button);
    logged_in_container.addSubview(&li_website_button);
    logged_in_container.addSubview(&stop_button);
    logged_in_container.addSubview(&logout_button);
    logged_in_container.addSubview(&request_permissions_button);
    logged_in_container.addSubview(&relaunch_button);

    content.addSubview(&content_background);
    content.addSubview(&sidebar_background);
    content.addSubview(&logo_view);
    content.addSubview(&login_container);
    content.addSubview(&logged_in_container);

    let proxy = MAIN_WINDOW_EVENT_PROXY
        .get()
        .cloned()
        .context("main window event proxy not initialized")?;

    controller.set_all(
        window.clone(),
        proxy,
        login_container.clone(),
        login_email_field.clone(),
        login_password_field.clone(),
        login_error_label.clone(),
        logged_in_container.clone(),
        message_label.clone(),
        permission_label.clone(),
        request_permissions_button.clone(),
        relaunch_button.clone(),
    );

    // Set initial mode visibility.
    match &details.mode {
        MainWindowMode::Login { .. } => {
            logged_in_container.setHidden(true);
        }
        MainWindowMode::LoggedIn { email, phase } => {
            login_container.setHidden(true);
            let msg = format!("Version: {}\nSigned in as {}", details.build_label, email);
            controller.switch_to_logged_in_mode(&msg, *phase);
        }
    }

    window.setDelegate(Some(ProtocolObject::from_ref(&*controller)));
    show_modeless_window(mtm, &window);

    if matches!(details.mode, MainWindowMode::Login { .. }) {
        let _ = window.makeFirstResponder(Some(&login_email_field));
    }

    Ok(MainWindowHandle { controller, window })
}

pub fn confirm_stop_monitoring() -> Result<bool> {
    confirm_action(
        "Virtue stop monitoring",
        "Stopping the background service will alert people monitoring you. Reopen the Virtue app to restart monitoring. Continue?",
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

pub fn show_error(message: &str) -> Result<()> {
    show_alert("Operation failed", message, NSAlertStyle::Critical)?;
    Ok(())
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

fn open_url(url: &str) -> Result<()> {
    let status = Command::new("open")
        .arg(url)
        .status()
        .context("failed to launch URL opener")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("URL opener exited with status {status}"))
    }
}

fn open_virtue_website() -> Result<()> {
    open_url(VIRTUE_WEBSITE_URL)
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
