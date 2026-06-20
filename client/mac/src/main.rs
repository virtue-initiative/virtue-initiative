mod capture;
mod config;
mod daemon;
mod launch_agent;
mod runtime_env;
mod ui;

use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use virtue_core::{AuthState, ClientController, CoreError, ServiceStatus};

use crate::capture::{has_screen_capture_access, request_screen_capture_access};
use crate::config::{
    ClientPaths, ClientState, build_core_config, default_device_name, load_state, save_state,
};
use crate::runtime_env::apply_runtime_env;

const BUILD_LABEL: &str = virtue_core::BUILD_LABEL;
const SERVICE_START_TIMEOUT: Duration = Duration::from_secs(20);
const SERVICE_START_POLL_INTERVAL: Duration = Duration::from_millis(100);
const STATUS_POLL_INTERVAL: Duration = Duration::from_secs(1);
const POST_RELAUNCH_GRACE: Duration = Duration::from_secs(30);

#[derive(Debug, Parser)]
#[command(name = "virtue-mac")]
#[command(about = "Virtue macOS tray client")]
#[command(version = BUILD_LABEL)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Daemon,
    Status,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;
    apply_runtime_env(&paths);

    match cli.command {
        Some(Commands::Daemon) => daemon::run_daemon(&paths),
        Some(Commands::Status) => status(paths),
        None => run_tray(paths),
    }
}

// -- Tray menu -----------------------------------------------------------------

struct TrayMenu {
    open_item: MenuItem,
    login_item: Option<MenuItem>,
    logout_item: Option<MenuItem>,
    quit_item: MenuItem,
}

fn build_tray_menu(logged_in: bool) -> (Menu, TrayMenu) {
    let menu = Menu::new();
    let open_item = MenuItem::new("Open Virtue", true, None);
    let _ = menu.append(&open_item);
    let _ = menu.append(&PredefinedMenuItem::separator());

    let login_item = if !logged_in {
        let item = MenuItem::new("Log In", true, None);
        let _ = menu.append(&item);
        Some(item)
    } else {
        None
    };
    let logout_item = if logged_in {
        let item = MenuItem::new("Logout", true, None);
        let _ = menu.append(&item);
        Some(item)
    } else {
        None
    };

    let _ = menu.append(&PredefinedMenuItem::separator());
    let quit_label = if logged_in {
        "Stop Monitoring and Quit"
    } else {
        "Quit"
    };
    let quit_item = MenuItem::new(quit_label, true, None);
    let _ = menu.append(&quit_item);

    (
        menu,
        TrayMenu {
            open_item,
            login_item,
            logout_item,
            quit_item,
        },
    )
}

// -- Tray event loop -----------------------------------------------------------

fn run_tray(paths: ClientPaths) -> Result<()> {
    // Try to start the daemon. If we can't confirm it's running, fail hard with a modal.
    if let Ok(exe) = std::env::current_exe()
        && let Err(err) = launch_agent::ensure_agent_running(&paths, &exe)
    {
        eprintln!("warning: launch agent setup failed: {err:#}");
    }
    if let Err(err) = ensure_background_service_running(&paths) {
        eprintln!("error: {err:#}");
        let _ = ui::show_error(
            "Virtue could not connect to the background monitoring service.\n\n\
             Please try relaunching the Virtue app.",
        );
        return Err(err);
    }

    let event_loop = EventLoopBuilder::<ui::MainWindowEvent>::with_user_event().build();
    ui::install_main_window_event_proxy(event_loop.create_proxy());

    let initially_logged_in = read_auth_state(&paths.state_dir)
        .map(|a| a.device_credentials.is_some())
        .unwrap_or(false);
    let (initial_menu, mut tray_menu) = build_tray_menu(initially_logged_in);
    let mut tray_logged_in = initially_logged_in;

    let tray_icon = TrayIconBuilder::new()
        .with_tooltip(format!("Virtue {BUILD_LABEL} - virtueinitiative.org"))
        .with_icon(build_tray_icon()?)
        .with_icon_as_template(true)
        .with_menu_on_left_click(false)
        .with_menu(Box::new(initial_menu))
        .build()
        .context("failed to build tray icon")?;

    let mut main_window = None;
    let mut next_status_poll_at = Instant::now();
    let mut relaunching = false;
    // Set to true when we intentionally stop the daemon so the poll thread's
    // result doesn't trigger the unexpected-exit message.
    let mut graceful_shutdown = false;
    // Timestamp of the first consecutive "Stopped" (connection-refused) poll.
    // We only declare the daemon gone when the socket refuses connections for
    // STOPPED_TIMEOUT, which tolerates a brief launchd restart race. A daemon
    // that accepts the connection but is slow to answer (Unreachable — e.g.
    // busy flushing a batch after wake) is alive and never triggers this.
    let mut stopped_since: Option<Instant> = None;
    const STOPPED_TIMEOUT: Duration = Duration::from_secs(20);
    // After a relaunch completes, give the new daemon time to settle before the
    // outage timer starts.
    let mut post_relaunch_grace_until: Option<Instant> = None;
    // True while a poll thread is running. Polls open a fresh daemon connection,
    // so spawning a new one before the last finished would pile up connections
    // (each get_status blocks up to 10s) and exhaust the daemon's file
    // descriptors. Only ever keep one poll in flight.
    let mut poll_in_flight = false;

    if let Err(err) = open_app_dialog(&paths, &mut main_window) {
        eprintln!("initial dialog failed: {err:#}");
        let _ = ui::show_error(&format!("Operation failed:\n{err}"));
    }

    event_loop.run(move |event, _event_loop_target, control_flow| {
        if let Event::UserEvent(main_window_event) = event {
            if let ui::MainWindowEvent::StatusPolled {
                logged_in,
                status,
                has_capture_permission,
            } = main_window_event
            {
                // The in-flight poll has finished; allow the next one to spawn.
                poll_in_flight = false;
                // Ignore stale results captured around a relaunch or an
                // intentional shutdown. (Login and logout run synchronously on
                // this thread, so no poll can spawn or be processed during them.)
                if relaunching
                    || graceful_shutdown
                    || post_relaunch_grace_until.is_some_and(|t| Instant::now() < t)
                {
                    stopped_since = None;
                    return;
                }
                match status {
                    // Connected but slow to answer — daemon is alive but busy
                    // (e.g. flushing a batch after wake). Never treat as gone.
                    ui::DaemonStatus::Unreachable => {
                        stopped_since = None;
                        return;
                    }
                    // Connection refused — the socket isn't accepting. Give it
                    // STOPPED_TIMEOUT to tolerate a launchd restart race.
                    ui::DaemonStatus::Stopped => {
                        let since = *stopped_since.get_or_insert_with(Instant::now);
                        if since.elapsed() < STOPPED_TIMEOUT {
                            return;
                        }
                        let _ = ui::show_info(
                            "Virtue background service stopped unexpectedly.\n\n\
                             Relaunch the Virtue app to continue monitoring.",
                        );
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                    ui::DaemonStatus::Running => {
                        stopped_since = None;
                    }
                }
                if logged_in != tray_logged_in {
                    tray_logged_in = logged_in;
                    let (new_menu, new_tray_menu) = build_tray_menu(logged_in);
                    tray_menu = new_tray_menu;
                    tray_icon.set_menu(Some(Box::new(new_menu)));
                }
                if has_capture_permission && let Some(window) = main_window.as_ref() {
                    window.update_permission_phase(None);
                }
            } else {
                match handle_main_window_event(
                    &paths,
                    &mut main_window,
                    main_window_event,
                    &mut relaunching,
                    &mut graceful_shutdown,
                    &mut post_relaunch_grace_until,
                ) {
                    Ok(true) => {
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                    Ok(false) => {
                        next_status_poll_at = Instant::now();
                    }
                    Err(err) => {
                        eprintln!("main window action failed: {err:#}");
                        let _ = ui::show_error(&format!("Operation failed:\n{err}"));
                    }
                }
            }
        }

        while let Ok(menu_event) = MenuEvent::receiver().try_recv() {
            let id = menu_event.id;
            if id == tray_menu.open_item.id()
                || tray_menu.login_item.as_ref().is_some_and(|i| id == i.id())
            {
                if let Err(err) = open_app_dialog(&paths, &mut main_window) {
                    eprintln!("open dialog failed: {err:#}");
                    let _ = ui::show_error(&format!("Operation failed:\n{err}"));
                }
                next_status_poll_at = Instant::now();
            } else if tray_menu.logout_item.as_ref().is_some_and(|i| id == i.id()) {
                match ui::confirm_logout() {
                    Ok(true) => {
                        let saved_email = load_state(&paths.ui_state_file)
                            .ok()
                            .and_then(|s| s.email)
                            .unwrap_or_default();
                        // This runs synchronously on the event-loop thread, so
                        // no status poll can spawn or be processed during it.
                        let result = logout(&paths);
                        stopped_since = None;
                        match result {
                            Ok(()) => {
                                if let Some(window) = main_window.as_ref() {
                                    window.switch_to_login(&saved_email, &default_device_name());
                                }
                            }
                            Err(err) => {
                                eprintln!("logout failed: {err:#}");
                                let _ = ui::show_error(&format!("Logout failed:\n{err}"));
                            }
                        }
                    }
                    Ok(false) => {}
                    Err(err) => eprintln!("confirm failed: {err:#}"),
                }
                next_status_poll_at = Instant::now();
            } else if id == tray_menu.quit_item.id() {
                graceful_shutdown = true;
                if agent_is_registered(&paths) {
                    // When logged in this item is "Stop Monitoring and Quit" — a
                    // user-initiated stop, so signal it as such.
                    let _ = stop_background_service(&paths, tray_logged_in);
                }
                *control_flow = ControlFlow::Exit;
                return;
            }
        }

        while let Ok(tray_event) = TrayIconEvent::receiver().try_recv() {
            if matches!(
                tray_event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                if let Err(err) = open_app_dialog(&paths, &mut main_window) {
                    eprintln!("open dialog failed: {err:#}");
                    let _ = ui::show_error(&format!("Operation failed:\n{err}"));
                }
                next_status_poll_at = Instant::now();
            }
        }

        // Spawn a background thread per poll tick so IPC never blocks the main
        // thread. Skip while relaunching (the daemon is being restarted) or while
        // a previous poll is still running, so connections never pile up.
        if !relaunching && !poll_in_flight && Instant::now() >= next_status_poll_at {
            let paths = paths.clone();
            poll_in_flight = true;
            thread::spawn(move || {
                let status = poll_daemon_status(&paths);
                let running = status == ui::DaemonStatus::Running;
                let logged_in = if running {
                    read_auth_state(&paths.state_dir)
                        .map(|a| a.device_credentials.is_some())
                        .unwrap_or(false)
                } else {
                    false
                };
                let has_capture_permission = running && has_screen_capture_access();
                let _ = ui::send_main_window_event(ui::MainWindowEvent::StatusPolled {
                    logged_in,
                    status,
                    has_capture_permission,
                });
            });
            next_status_poll_at = Instant::now() + STATUS_POLL_INTERVAL;
        }

        *control_flow = ControlFlow::WaitUntil(next_status_poll_at);
    });
}

// -- App dialog ----------------------------------------------------------------

fn open_app_dialog(
    paths: &ClientPaths,
    main_window: &mut Option<ui::MainWindowHandle>,
) -> Result<()> {
    if let Some(window) = main_window.as_ref() {
        window.focus()?;
        return Ok(());
    }

    let app_status = collect_status(paths)?;
    let default_email = app_status.email.clone().unwrap_or_default();
    let default_device = default_device_name();
    let email_str = app_status
        .email
        .as_deref()
        .unwrap_or("<unknown>")
        .to_string();

    let details = if !app_status.logged_in {
        ui::MainWindowDetails {
            build_label: BUILD_LABEL,
            mode: ui::MainWindowMode::Login {
                default_email: &default_email,
                default_device_name: &default_device,
            },
        }
    } else {
        let phase = permission_phase(app_status.has_capture_permission);
        ui::MainWindowDetails {
            build_label: BUILD_LABEL,
            mode: ui::MainWindowMode::LoggedIn {
                email: &email_str,
                phase,
            },
        }
    };

    *main_window = Some(ui::show_main_window(&details)?);
    Ok(())
}

// -- Main window event handler -------------------------------------------------

fn handle_main_window_event(
    paths: &ClientPaths,
    main_window: &mut Option<ui::MainWindowHandle>,
    event: ui::MainWindowEvent,
    relaunching: &mut bool,
    graceful_shutdown: &mut bool,
    post_relaunch_grace_until: &mut Option<Instant>,
) -> Result<bool> {
    match event {
        ui::MainWindowEvent::Closed => {
            *main_window = None;
            Ok(false)
        }
        ui::MainWindowEvent::LoginSubmitted {
            email,
            password,
            device_name,
        } => {
            // Runs synchronously on the event-loop thread; the app intentionally
            // blocks (and no status poll spawns or is processed) until login
            // resolves, since the daemon is busy with the login network call.
            match login(paths, &email, &password, &device_name) {
                Ok(_) => {
                    // Do not auto-request screen-capture access here: macOS shows
                    // its prompt only once per launch, so triggering it now would
                    // consume the one-shot before the user clicks the explicit
                    // "Request Permissions" button.
                    let phase = match collect_status(paths) {
                        Ok(s) => permission_phase(s.has_capture_permission),
                        Err(_) => Some(ui::PermissionPhase::NeedsRequest),
                    };
                    if let Some(window) = main_window.as_ref() {
                        window.switch_to_logged_in(&email, BUILD_LABEL, phase);
                    }
                }
                Err(e) => {
                    if let Some(window) = main_window.as_ref() {
                        window.show_login_error(&login_error_message(&e));
                    }
                }
            }
            Ok(false)
        }
        ui::MainWindowEvent::Action(ui::LoggedInAction::Status) => {
            ui::show_status(&render_status_text(paths)?)?;
            Ok(false)
        }
        ui::MainWindowEvent::Action(ui::LoggedInAction::RequestPermissions) => {
            let granted = request_screen_capture_access();
            if let Some(window) = main_window.as_ref() {
                if granted {
                    window.update_permission_phase(None);
                } else {
                    window.update_permission_phase(Some(ui::PermissionPhase::NeedsRelaunch));
                }
            }
            Ok(false)
        }
        ui::MainWindowEvent::Action(ui::LoggedInAction::RelaunchToAcceptPermissions) => {
            *relaunching = true;
            if let Some(window) = main_window.as_ref() {
                window.set_relaunch_button_state("Restarting...", false);
            }
            let paths = paths.clone();
            thread::spawn(move || {
                let result = relaunch_background_service(&paths);
                let error = result.err().map(|e| e.to_string());
                let _ = ui::send_main_window_event(ui::MainWindowEvent::RelaunchDone(error));
            });
            Ok(false)
        }
        ui::MainWindowEvent::RelaunchDone(error) => {
            *relaunching = false;
            if let Some(window) = main_window.as_ref() {
                window.set_relaunch_button_state("Relaunch to Accept Permissions", true);
            }
            match error {
                None => {
                    *post_relaunch_grace_until = Some(Instant::now() + POST_RELAUNCH_GRACE);
                    // Daemon restarted successfully, so the permission is now active.
                    // Update the UI directly rather than waiting for a poll — the
                    // tray process itself doesn't restart, so the local TCC query
                    // may still return false even though the daemon has the permission.
                    if let Some(window) = main_window.as_ref() {
                        window.update_permission_phase(None);
                    }
                    ui::show_info("Virtue background service relaunched.")?;
                }
                Some(msg) => {
                    ui::show_error(&format!("Relaunch failed:\n{msg}"))?;
                }
            }
            Ok(false)
        }
        ui::MainWindowEvent::Action(ui::LoggedInAction::StopMonitoring) => {
            if !ui::confirm_stop_monitoring()? {
                return Ok(false);
            }
            *graceful_shutdown = true;
            if agent_is_registered(paths) {
                // User-initiated stop: signal it so the daemon records a clean
                // user stop (alert at stop time, not an unexpected-start alert).
                stop_background_service(paths, true)
                    .context("failed to stop background service")?;
            }
            if let Some(window) = main_window.take() {
                window.close();
            }
            Ok(true)
        }
        ui::MainWindowEvent::Action(ui::LoggedInAction::Logout) => {
            if !ui::confirm_logout()? {
                return Ok(false);
            }
            let saved_email = load_state(&paths.ui_state_file)
                .ok()
                .and_then(|s| s.email)
                .unwrap_or_default();
            // Runs synchronously on the event-loop thread, so no status poll
            // can spawn or be processed during the logout IPC call.
            logout(paths)?;
            if let Some(window) = main_window.as_ref() {
                window.switch_to_login(&saved_email, &default_device_name());
            }
            Ok(false)
        }
        // Handled inline in the event loop; should never reach here.
        ui::MainWindowEvent::StatusPolled { .. } => Ok(false),
    }
}

// -- Helpers -------------------------------------------------------------------

fn permission_phase(has_capture_permission: bool) -> Option<ui::PermissionPhase> {
    if has_capture_permission {
        None
    } else {
        Some(ui::PermissionPhase::NeedsRequest)
    }
}

fn login(paths: &ClientPaths, email: &str, password: &str, device_name: &str) -> Result<String> {
    let sock = paths.state_dir.join("daemon.sock");
    let mut client =
        ClientController::connect(&sock).context("failed to connect to daemon (is it running?)")?;
    let device_id = client
        .login(email, password, Some(device_name))
        .context("login failed")?;
    save_state(
        &paths.ui_state_file,
        &ClientState {
            email: Some(email.to_string()),
        },
    )?;
    Ok(device_id)
}

fn login_error_message(err: &anyhow::Error) -> String {
    for cause in err.chain() {
        if let Some(core_err) = cause.downcast_ref::<CoreError>() {
            if core_err.is_unauthorized() || core_err.is_bad_request() {
                return "Login failed. Check your email and password and try again.".to_string();
            }
            return format!("Login failed: {core_err}");
        }
    }
    match err.root_cause().to_string() {
        message if message.trim().is_empty() || message == "login failed" => {
            "Login failed. Try again.".to_string()
        }
        message => format!("Login failed: {message}"),
    }
}

fn logout(paths: &ClientPaths) -> Result<()> {
    let sock = paths.state_dir.join("daemon.sock");
    let mut client =
        ClientController::connect(&sock).context("failed to connect to daemon (is it running?)")?;
    client.logout().context("logout failed")?;
    // Do NOT stop the daemon — the tray remains open and the user can log back in.
    save_state(&paths.ui_state_file, &ClientState { email: None })?;
    Ok(())
}

fn agent_is_registered(paths: &ClientPaths) -> bool {
    paths.launch_agent_file.exists() || launch_agent::is_agent_loaded().unwrap_or(false)
}

/// Stop the background service. When `user_initiated` is true, first tell the
/// daemon a user requested the stop so it records a clean user stop (which fires
/// a stop-time alert) instead of being classified as an unexpected `Other` stop
/// that would trigger an unexpected-start alert on the next launch.
fn stop_background_service(paths: &ClientPaths, user_initiated: bool) -> Result<()> {
    if user_initiated {
        signal_user_stop(paths);
    }
    launch_agent::stop_agent(paths)
}

/// Send `UserStopRequested` to the daemon and wait for a status round-trip on
/// the same connection. Because the daemon's reader thread dispatches inbound
/// lines in order, a returned status response guarantees `UserStopRequested`
/// was already processed before we deliver SIGTERM via `bootout`.
fn signal_user_stop(paths: &ClientPaths) {
    let sock = paths.state_dir.join("daemon.sock");
    if let Ok(mut client) = ClientController::connect(&sock)
        && client.request_user_stop("mac_tray_stop").is_ok()
    {
        let _ = client.get_status();
    }
}

fn ensure_background_service_running(paths: &ClientPaths) -> Result<()> {
    let deadline = Instant::now() + SERVICE_START_TIMEOUT;
    loop {
        // The daemon is "up" as soon as its socket accepts connections, even if
        // it is too busy to answer a status request promptly. Right after a macOS
        // session login the freshly-started daemon floods through its startup
        // backlog (lifecycle alerts, screenshot capture, uploads), which blocks
        // it from answering `get_status` for a while — but it is running. Only a
        // refused connection (Stopped) means it has not started yet.
        if !matches!(poll_daemon_status(paths), ui::DaemonStatus::Stopped) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow::anyhow!(
                "background service did not start within {} seconds",
                SERVICE_START_TIMEOUT.as_secs()
            ));
        }
        thread::sleep(SERVICE_START_POLL_INTERVAL);
    }
}

/// Poll the daemon, distinguishing a refused connection (daemon genuinely
/// stopped) from a timeout/IPC error (daemon alive but busy). A successful
/// status response always means running, since the lifecycle module hardcodes
/// `is_running: true` whenever it can answer a `StatusRequest`.
fn poll_daemon_status(paths: &ClientPaths) -> ui::DaemonStatus {
    let sock = paths.state_dir.join("daemon.sock");
    match ClientController::connect(&sock) {
        Err(_) => ui::DaemonStatus::Stopped,
        Ok(mut client) => match client.get_status() {
            Ok(_) => ui::DaemonStatus::Running,
            Err(_) => ui::DaemonStatus::Unreachable,
        },
    }
}

fn relaunch_background_service(paths: &ClientPaths) -> Result<()> {
    if agent_is_registered(paths) {
        launch_agent::stop_agent(paths)
            .context("failed to stop existing background service before relaunch")?;
    }
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    launch_agent::ensure_agent_running(paths, &exe)
        .context("failed to relaunch background service")?;
    ensure_background_service_running(paths)?;
    Ok(())
}

fn status(paths: ClientPaths) -> Result<()> {
    println!("{}", render_status_text(&paths)?);
    Ok(())
}

fn render_status_text(paths: &ClientPaths) -> Result<String> {
    let auth = read_auth_state(&paths.state_dir)?;
    let service_status = load_service_status(paths, &auth)?;
    let mut config = build_core_config(paths);
    config.refresh_from_runtime_file()?;
    let mut lines = Vec::new();
    lines.push(format!("logged_in: {}", auth.device_credentials.is_some()));
    lines.push(format!("running: {}", service_status.is_running));
    lines.push(format!(
        "pending_request_count: {}",
        service_status.pending_request_count
    ));
    lines.push(format!(
        "device_id: {}",
        service_status.device_id.as_deref().unwrap_or("<none>")
    ));
    lines.push(format!(
        "capture_interval_seconds: {}",
        config.screenshot_interval.as_secs()
    ));
    lines.push(format!(
        "batch_window_seconds: {}",
        config.batch_interval.as_secs()
    ));
    lines.push(format!("base_api_url: {}", config.api_base_url));
    lines.push("backend: screencapture".to_string());
    Ok(lines.join("\n"))
}

#[derive(Debug)]
struct AppStatus {
    logged_in: bool,
    email: Option<String>,
    has_capture_permission: bool,
}

fn collect_status(paths: &ClientPaths) -> Result<AppStatus> {
    let state = load_state(&paths.ui_state_file)?;
    let auth = read_auth_state(&paths.state_dir)?;
    Ok(AppStatus {
        logged_in: auth.device_credentials.is_some(),
        email: state.email,
        // Same TCC permission applies to the tray and daemon (same bundle), so
        // check directly rather than asking the daemon.
        has_capture_permission: has_screen_capture_access(),
    })
}

fn read_auth_state(state_dir: &std::path::Path) -> Result<AuthState> {
    let path = state_dir.join("event_state.json");
    if !path.exists() {
        return Ok(AuthState::default());
    }
    let bytes = std::fs::read(&path)?;
    let state: serde_json::Value = serde_json::from_slice(&bytes)?;
    if let Some(auth) = state.get("auth")
        && !auth.is_null()
    {
        return Ok(serde_json::from_value(auth.clone())?);
    }
    Ok(AuthState::default())
}

fn load_service_status(paths: &ClientPaths, auth: &AuthState) -> Result<ServiceStatus> {
    let sock = paths.state_dir.join("daemon.sock");
    if let Ok(mut client) = ClientController::connect(&sock)
        && let Ok(status) = client.get_status()
    {
        return Ok(status);
    }
    Ok(ServiceStatus {
        is_authenticated: auth.device_credentials.is_some(),
        is_running: false,
        device_id: auth
            .device_credentials
            .as_ref()
            .map(|d| d.device_id.clone()),
        last_loop_at_ms: None,
        pending_request_count: 0,
    })
}

fn build_tray_icon() -> Result<Icon> {
    let png_bytes = include_bytes!("../assets/tray-icon.png");
    let image = image::load_from_memory(png_bytes)
        .context("failed to decode tray icon image")?
        .into_rgba8();
    let (width, height) = image.dimensions();
    let rgba = image.into_raw();
    Icon::from_rgba(rgba, width, height).context("failed to build tray icon")
}
