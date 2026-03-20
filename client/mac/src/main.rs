mod capture;
mod config;
mod daemon;
mod launch_agent;
mod runtime_env;
mod ui;

use std::process::ExitCode;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use virtue_core::storage::FileStateStore;
use virtue_core::{AuthState, LogEntry, MonitorService, ServiceStatus};

use crate::capture::MacPlatformHooks;
use crate::config::{
    ClientPaths, ClientState, ScreenshotPermissionStatus, build_core_config, load_daemon_status,
    load_state, save_state,
};
use crate::runtime_env::apply_runtime_env;

const BUILD_LABEL: &str = env!("CARGO_PKG_VERSION");

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
        Some(Commands::Daemon) => {
            let runtime = tokio::runtime::Runtime::new().context("failed to create runtime")?;
            runtime.block_on(daemon::run_daemon(&paths))
        }
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

    let event_loop = EventLoopBuilder::<()>::with_user_event().build();

    let menu = Menu::new();
    let open_item = MenuItem::new("Open Virtue", true, None);
    let close_item = MenuItem::new("Close (Will Send Alert)", true, None);
    menu.append(&open_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&close_item)?;

    let _tray_icon = TrayIconBuilder::new()
        .with_tooltip(format!("Virtue {BUILD_LABEL}"))
        .with_icon(build_tray_icon()?)
        .with_menu_on_left_click(false)
        .with_menu(Box::new(menu))
        .build()
        .context("failed to build tray icon")?;

    if let Err(err) = open_app_dialog(&paths) {
        eprintln!("initial dialog failed: {err:#}");
        let _ = ui::show_error(&format!("Operation failed:\n{err}"));
    }

    event_loop.run(move |event, _event_loop_target, control_flow| {
        *control_flow = ControlFlow::Wait;

        if matches!(event, Event::NewEvents(StartCause::Init)) {}

        while let Ok(menu_event) = MenuEvent::receiver().try_recv() {
            if menu_event.id == close_item.id() {
                if let Err(err) = close_tray_and_service(&paths) {
                    eprintln!("close failed: {err:#}");
                    let _ = ui::show_error(&format!("Could not close background service:\n{err}"));
                    continue;
                }
                *control_flow = ControlFlow::Exit;
                return;
            }

            if menu_event.id == open_item.id()
                && let Err(err) = open_app_dialog(&paths)
            {
                eprintln!("open dialog failed: {err:#}");
                let _ = ui::show_error(&format!("Operation failed:\n{err}"));
            }
        }

        while let Ok(tray_event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = tray_event
                && let Err(err) = open_app_dialog(&paths)
            {
                eprintln!("open dialog failed: {err:#}");
                let _ = ui::show_error(&format!("Operation failed:\n{err}"));
            }
        }
    });
}

fn close_tray_and_service(paths: &ClientPaths) -> Result<()> {
    if let Err(err) = send_close_alert(paths) {
        eprintln!("warning: could not send close alert: {err:#}");
    }
    launch_agent::stop_agent(paths).context("failed to stop background service")
}

fn open_app_dialog(paths: &ClientPaths) -> Result<()> {
    let app_status = collect_status(paths)?;
    if app_status.logged_in {
        let email = app_status.email.as_deref().unwrap_or("<unknown>");
        let device_id = app_status.device_id.as_deref().unwrap_or("<unknown>");
        let dialog_details = ui::LoggedInDialogDetails {
            build_label: BUILD_LABEL,
            email,
            device_id,
            monitor_summary: &app_status.monitor_summary,
            pending_request_count: app_status.pending_request_count,
            base_api_url: &app_status.base_api_url,
            screenshot_permission: app_status.screenshot_permission.as_str(),
            daemon_status_updated_at: app_status.daemon_status_updated_at.as_deref(),
            daemon_last_error: app_status.daemon_last_error.as_deref(),
        };
        let action = if app_status.screenshot_permission == ScreenshotPermissionStatus::Missing {
            ui::prompt_permission_issue_action(&dialog_details)?
        } else {
            ui::prompt_logged_in_action(&dialog_details)?
        };

        match action {
            Some(ui::LoggedInAction::RestartDaemon) => {
                restart_daemon(paths)?;
                ui::show_info("Background service restarted.")?;
            }
            Some(ui::LoggedInAction::Logout) => {
                logout(paths)?;
                ui::show_info("Signed out. Monitoring disabled on this device.")?;
            }
            _ => {}
        }
        return Ok(());
    }

    let Some(input) = ui::prompt_login(BUILD_LABEL, app_status.email.as_deref())? else {
        return Ok(());
    };

    let device_id = login(paths, &input.email, &input.password)?;
    ui::show_info(&format!("Signed in.\nDevice id: {device_id}"))?;
    Ok(())
}

fn restart_daemon(paths: &ClientPaths) -> Result<()> {
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    launch_agent::ensure_agent_running(paths, &exe).context("failed to restart background service")
}

fn login(paths: &ClientPaths, email: &str, password: &str) -> Result<String> {
    let mut service = MonitorService::setup(build_core_config(paths), MacPlatformHooks::new())?;
    let login_result = service.login(email, password).context("login failed")?;
    save_state(
        &paths.ui_state_file,
        &ClientState {
            email: Some(email.to_string()),
        },
    )?;
    Ok(login_result
        .device
        .as_ref()
        .map(|device| device.device_id.clone())
        .unwrap_or_else(|| "<unknown>".to_string()))
}

fn logout(paths: &ClientPaths) -> Result<()> {
    let mut service = MonitorService::setup(build_core_config(paths), MacPlatformHooks::new())?;
    service.logout()?;
    save_state(&paths.ui_state_file, &ClientState { email: None })?;
    Ok(())
}

fn send_close_alert(paths: &ClientPaths) -> Result<()> {
    let store = FileStateStore::new(&paths.state_dir)?;
    let auth = store.load_auth_state()?;
    if auth.device_credentials.is_none() {
        return Ok(());
    }

    let mut service = MonitorService::setup(build_core_config(paths), MacPlatformHooks::new())?;
    let _ = service.send_log(LogEntry {
        ts_ms: Utc::now().timestamp_millis(),
        kind: "manual_override".to_string(),
        risk: None,
        data: serde_json::json!({
            "source": "mac_tray_menu",
            "reason": "tray_close_requested",
        }),
    });
    Ok(())
}

fn status(paths: ClientPaths) -> Result<()> {
    let store = FileStateStore::new(&paths.state_dir)?;
    let auth = store.load_auth_state()?;
    let service_status = load_service_status(&store, &auth)?;
    let device_settings = store.load_device_settings()?;
    let daemon_status = load_daemon_status(&paths.daemon_status_file)?;
    let mut config = build_core_config(&paths);
    config.refresh_from_runtime_file()?;

    println!("logged_in: {}", auth.device_credentials.is_some());
    println!("running: {}", service_status.is_running);
    println!(
        "pending_request_count: {}",
        service_status.pending_request_count
    );
    println!(
        "device_id: {}",
        service_status.device_id.as_deref().unwrap_or("<none>")
    );
    println!(
        "device_enabled: {}",
        device_settings
            .as_ref()
            .map(|settings| settings.enabled.to_string())
            .unwrap_or_else(|| "<unknown>".to_string())
    );
    println!(
        "screenshot_permission: {}",
        daemon_status.screenshot_permission.as_str()
    );
    println!(
        "daemon_last_error: {}",
        daemon_status.last_error.as_deref().unwrap_or("<none>")
    );
    println!(
        "daemon_status_updated_at: {}",
        daemon_status.updated_at.as_deref().unwrap_or("<none>")
    );
    println!(
        "capture_interval_seconds: {}",
        config.screenshot_interval.as_secs()
    );
    println!("batch_window_seconds: {}", config.batch_interval.as_secs());
    println!("base_api_url: {}", config.api_base_url);
    Ok(())
}

#[derive(Debug)]
struct AppStatus {
    logged_in: bool,
    email: Option<String>,
    device_id: Option<String>,
    monitor_summary: String,
    pending_request_count: usize,
    base_api_url: String,
    screenshot_permission: ScreenshotPermissionStatus,
    daemon_last_error: Option<String>,
    daemon_status_updated_at: Option<String>,
}

fn collect_status(paths: &ClientPaths) -> Result<AppStatus> {
    let store = FileStateStore::new(&paths.state_dir)?;
    let state = load_state(&paths.ui_state_file)?;
    let auth = store.load_auth_state()?;
    let service_status = load_service_status(&store, &auth)?;
    let device_settings = store.load_device_settings()?;
    let daemon_status = load_daemon_status(&paths.daemon_status_file)?;
    let mut config = build_core_config(paths);
    config.refresh_from_runtime_file()?;

    let monitor_summary = if auth.device_credentials.is_none() {
        "signed out".to_string()
    } else if device_settings.as_ref().is_some_and(|settings| !settings.enabled) {
        "disabled by device settings".to_string()
    } else if service_status.is_running {
        "active".to_string()
    } else {
        "background service not running".to_string()
    };

    Ok(AppStatus {
        logged_in: auth.device_credentials.is_some(),
        email: state.email,
        device_id: auth
            .device_credentials
            .as_ref()
            .map(|device| device.device_id.clone()),
        monitor_summary,
        pending_request_count: service_status.pending_request_count,
        base_api_url: config.api_base_url,
        screenshot_permission: daemon_status.screenshot_permission,
        daemon_last_error: daemon_status.last_error,
        daemon_status_updated_at: daemon_status.updated_at,
    })
}

fn load_service_status(store: &FileStateStore, auth: &AuthState) -> Result<ServiceStatus> {
    let pending_request_count = store.load_pending_requests()?.len();
    Ok(store.load_status()?.unwrap_or(ServiceStatus {
        is_authenticated: auth.device_credentials.is_some(),
        is_running: false,
        device_id: auth
            .device_credentials
            .as_ref()
            .map(|device| device.device_id.clone()),
        last_loop_at_ms: None,
        last_screenshot_at_ms: None,
        last_batch_at_ms: None,
        pending_request_count,
    }))
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
