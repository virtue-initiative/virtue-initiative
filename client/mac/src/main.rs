mod capture;
mod capture_reporter;
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
use virtue_core::{AuthState, ControllerClient, CoreError, ServiceStatus};

use crate::capture::{MacEvent, has_screen_capture_access, open_screen_capture_settings, request_screen_capture_access};
use crate::config::{ClientPaths, ClientState, build_core_config, load_state, save_state};
use crate::runtime_env::apply_runtime_env;

const BUILD_LABEL: &str = virtue_core::BUILD_LABEL;
const SERVICE_START_TIMEOUT: Duration = Duration::from_secs(20);
const SERVICE_START_POLL_INTERVAL: Duration = Duration::from_millis(100);
const STATUS_POLL_INTERVAL: Duration = Duration::from_secs(1);

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

fn run_tray(paths: ClientPaths) -> Result<()> {
    if let Ok(exe) = std::env::current_exe()
        && let Err(err) = launch_agent::ensure_agent_running(&paths, &exe)
    {
        eprintln!("warning: launch agent setup failed: {err:#}");
        let _ = ui::show_warning(&format!(
            "Could not start background service automatically:\n{err}"
        ));
    }

    if let Err(err) = ensure_background_service_running(&paths) {
        eprintln!("warning: background service startup confirmation delayed: {err:#}");
        let _ = ui::show_warning(&format!(
            "Virtue is still waiting for the background service to finish starting.\n\n{err}\n\nThe tray app will stay open while the service keeps starting."
        ));
    }
    maybe_request_screen_capture_access_for_logged_in_user(&paths)?;

    let event_loop = EventLoopBuilder::<ui::MainWindowEvent>::with_user_event().build();
    ui::install_main_window_event_proxy(event_loop.create_proxy());

    let menu = Menu::new();
    let open_item = MenuItem::new("Open Virtue", true, None);
    let close_item = MenuItem::new("Stop Monitoring", true, None);
    menu.append(&open_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&close_item)?;

    let _tray_icon = TrayIconBuilder::new()
        .with_tooltip(format!("Virtue {BUILD_LABEL} - virtueinitiative.org"))
        .with_icon(build_tray_icon()?)
        .with_menu_on_left_click(false)
        .with_menu(Box::new(menu))
        .build()
        .context("failed to build tray icon")?;

    let mut main_window = None;
    let mut next_status_poll_at = Instant::now();
    let mut relaunching = false;

    if let Err(err) = open_app_dialog(&paths, &mut main_window) {
        eprintln!("initial dialog failed: {err:#}");
        let _ = ui::show_error(&format!("Operation failed:\n{err}"));
    }

    event_loop.run(move |event, _event_loop_target, control_flow| {
        if let Event::UserEvent(main_window_event) = event {
            match handle_main_window_event(&paths, &mut main_window, main_window_event, &mut relaunching) {
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

        while let Ok(menu_event) = MenuEvent::receiver().try_recv() {
            if menu_event.id == close_item.id() {
                match close_tray_and_service(&paths) {
                    Ok(true) => {
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                    Ok(false) => continue,
                    Err(err) => {
                        eprintln!("close failed: {err:#}");
                        let _ =
                            ui::show_error(&format!("Could not close background service:\n{err}"));
                        continue;
                    }
                }
            }

            if menu_event.id == open_item.id() {
                if let Err(err) = open_app_dialog(&paths, &mut main_window) {
                    eprintln!("open dialog failed: {err:#}");
                    let _ = ui::show_error(&format!("Operation failed:\n{err}"));
                }
                next_status_poll_at = Instant::now();
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

        if main_window.is_some() && !relaunching && Instant::now() >= next_status_poll_at {
            if let Err(err) = refresh_main_window_status(&paths, main_window.as_ref()) {
                eprintln!("main window status refresh failed: {err:#}");
            }
            next_status_poll_at = Instant::now() + STATUS_POLL_INTERVAL;
        }

        *control_flow = if main_window.is_some() {
            ControlFlow::WaitUntil(next_status_poll_at)
        } else {
            ControlFlow::Wait
        };
    });
}

fn close_tray_and_service(paths: &ClientPaths) -> Result<bool> {
    if agent_is_registered(paths) && !ui::confirm_stop_monitoring()? {
        return Ok(false);
    }

    let stopped = stop_monitoring(paths)?;
    if stopped {
        ui::show_info("Stopped monitoring. Open the Virtue app to start monitoring again.")?;
    }

    Ok(true)
}

fn open_app_dialog(
    paths: &ClientPaths,
    main_window: &mut Option<ui::MainWindowHandle>,
) -> Result<()> {
    let app_status = collect_status(paths)?;
    if !app_status.logged_in {
        let Some(device_id) =
            ui::prompt_login(BUILD_LABEL, app_status.email.as_deref(), |input| {
                login(paths, &input.email, &input.password).map_err(|err| login_error_message(&err))
            })?
        else {
            return Ok(());
        };
        request_screen_capture_access_for_monitoring()?;
        ui::show_info(&format!("Signed in.\nDevice id: {device_id}"))?;
    }

    if let Some(window) = main_window.as_ref() {
        window.focus()?;
        return Ok(());
    }

    let app_status = collect_status(paths)?;
    let email = app_status.email.as_deref().unwrap_or("<unknown>");
    let dialog_details = ui::LoggedInDialogDetails {
        build_label: BUILD_LABEL,
        email,
        show_permission_actions: !app_status.has_capture_permission,
    };
    *main_window = Some(ui::show_main_window(&dialog_details)?);
    Ok(())
}

fn handle_main_window_event(
    paths: &ClientPaths,
    main_window: &mut Option<ui::MainWindowHandle>,
    event: ui::MainWindowEvent,
    relaunching: &mut bool,
) -> Result<bool> {
    match event {
        ui::MainWindowEvent::Closed => {
            *main_window = None;
            Ok(false)
        }
        ui::MainWindowEvent::Action(ui::LoggedInAction::Status) => {
            ui::show_status(&render_status_text(paths)?)?;
            Ok(false)
        }
        ui::MainWindowEvent::Action(ui::LoggedInAction::AllowScreenCapture) => {
            request_screen_capture_access_for_monitoring()?;
            ui::show_info(
                "Requested screen capture access. If macOS does not show the inline prompt, Screen Recording settings should open.",
            )?;
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
                None => ui::show_info("Virtue background service relaunched.")?,
                Some(msg) => ui::show_error(&format!("Relaunch failed:\n{msg}"))?,
            }
            Ok(false)
        }
        ui::MainWindowEvent::Action(ui::LoggedInAction::StopMonitoring) => {
            if !ui::confirm_stop_monitoring()? {
                return Ok(false);
            }
            let stopped = stop_monitoring(paths)?;
            if stopped {
                if let Some(window) = main_window.take() {
                    window.close();
                }
                ui::show_info(
                    "Stopped monitoring. Open the Virtue app to start monitoring again.",
                )?;
            }
            Ok(true)
        }
        ui::MainWindowEvent::Action(ui::LoggedInAction::Logout) => {
            if !ui::confirm_logout()? {
                return Ok(false);
            }
            logout(paths)?;
            if let Some(window) = main_window.take() {
                window.close();
            }
            ui::show_info(
                "Logged out. Monitoring is disabled on this device until you open the Virtue app and log in again.",
            )?;
            Ok(false)
        }
    }
}

fn login(paths: &ClientPaths, email: &str, password: &str) -> Result<String> {
    let sock = paths.state_dir.join("daemon.sock");
    let mut client =
        ControllerClient::connect(&sock).context("failed to connect to daemon (is it running?)")?;
    let device_id = client.login(email, password).context("login failed")?;
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
        ControllerClient::connect(&sock).context("failed to connect to daemon (is it running?)")?;
    client.logout().context("logout failed")?;
    launch_agent::stop_agent(paths).context("failed to unregister background service")?;
    save_state(&paths.ui_state_file, &ClientState { email: None })?;
    Ok(())
}

fn stop_monitoring(paths: &ClientPaths) -> Result<bool> {
    if !agent_is_registered(paths) {
        return Ok(false);
    }

    let sock = paths.state_dir.join("daemon.sock");
    let mut client =
        ControllerClient::connect(&sock).context("failed to connect to daemon (is it running?)")?;
    client
        .request_user_stop("tray_stop_monitoring")
        .context("failed to record stop intent")?;

    launch_agent::stop_agent(paths).context("failed to stop background service")?;
    Ok(true)
}

fn agent_is_registered(paths: &ClientPaths) -> bool {
    paths.launch_agent_file.exists() || launch_agent::is_agent_loaded().unwrap_or(false)
}

fn ensure_background_service_running(paths: &ClientPaths) -> Result<()> {
    let deadline = Instant::now() + SERVICE_START_TIMEOUT;

    while Instant::now() < deadline {
        if service_is_running(paths)? {
            return Ok(());
        }
        thread::sleep(SERVICE_START_POLL_INTERVAL);
    }

    Err(anyhow::anyhow!(
        "background service did not report running within {} seconds",
        SERVICE_START_TIMEOUT.as_secs()
    ))
}

fn service_is_running(paths: &ClientPaths) -> Result<bool> {
    let sock = paths.state_dir.join("daemon.sock");
    Ok(ControllerClient::connect(&sock)
        .ok()
        .and_then(|mut c| c.get_status().ok())
        .map(|s| s.is_running)
        .unwrap_or(false))
}

fn maybe_request_screen_capture_access_for_logged_in_user(paths: &ClientPaths) -> Result<()> {
    let auth = read_auth_state(&paths.state_dir)?;
    if auth.device_credentials.is_some() && !has_screen_capture_access() {
        request_screen_capture_access_for_monitoring()?;
    }
    Ok(())
}

fn request_screen_capture_access_for_monitoring() -> Result<()> {
    if request_screen_capture_access() {
        return Ok(());
    }
    open_screen_capture_settings()
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

    // Get capture permission from daemon via IPC — tray never holds the TCC grant itself.
    let has_capture_permission = {
        let sock = paths.state_dir.join("daemon.sock");
        let mut available = false;
        if let Ok(mut client) = ControllerClient::connect(&sock) {
            let _ = client.get_status_with_handler::<MacEvent, _>(|ev| {
                let MacEvent::CaptureAvailabilityChanged(a) = ev;
                available = a;
            });
        }
        available
    };

    Ok(AppStatus {
        logged_in: auth.device_credentials.is_some(),
        email: state.email,
        has_capture_permission,
    })
}

fn refresh_main_window_status(
    paths: &ClientPaths,
    main_window: Option<&ui::MainWindowHandle>,
) -> Result<()> {
    let Some(main_window) = main_window else {
        return Ok(());
    };

    let app_status = collect_status(paths)?;
    main_window.update_permission_section(ui::PermissionSectionState {
        show_permission_actions: !app_status.has_capture_permission,
    });
    Ok(())
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
    if let Ok(mut client) = ControllerClient::connect(&sock)
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
            .map(|device| device.device_id.clone()),
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
