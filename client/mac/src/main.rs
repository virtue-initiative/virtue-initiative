mod capture;
mod config;
mod daemon;
mod launch_agent;
mod runtime_env;
mod ui;

use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tokio::runtime::Runtime;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use crate::config::{
    ClientPaths, ScreenshotPermissionStatus, load_daemon_status, load_state, save_state,
};
use crate::runtime_env::apply_runtime_env;
use virtue_client_core::{
    ApiClient, AuthClient, FileTokenStore, LoginCommandInput, TokenStore,
    login_and_register_device, logout_and_clear_tokens_with_alert, resolve_base_api_url,
    resolve_batch_window_seconds, resolve_capture_interval_seconds,
};

#[derive(Debug, Parser)]
#[command(name = "virtue-mac-client")]
#[command(about = "Virtue macOS tray client")]
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
        Some(Commands::Daemon) => run_daemon(paths),
        Some(Commands::Status) => status(paths),
        None => run_tray(paths),
    }
}

fn run_daemon(paths: ClientPaths) -> Result<()> {
    let runtime = Runtime::new().context("failed to create async runtime")?;
    runtime.block_on(daemon::run_daemon(&paths))
}

fn run_tray(paths: ClientPaths) -> Result<()> {
    let runtime = Runtime::new().context("failed to create async runtime")?;
    if let Ok(exe) = std::env::current_exe() {
        if let Err(err) = launch_agent::ensure_agent_running(&paths, &exe) {
            eprintln!("warning: launch agent setup failed: {err:#}");
            let _ = ui::show_warning(&format!(
                "Could not start background service automatically:\n{err}"
            ));
        }
    }

    let event_loop = EventLoopBuilder::<()>::with_user_event().build();

    let menu = Menu::new();
    let open_item = MenuItem::new("Open Virtue", true, None);
    let close_item = MenuItem::new("Close (Will Send Alert)", true, None);
    menu.append(&open_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&close_item)?;

    let _tray_icon = TrayIconBuilder::new()
        .with_tooltip("Virtue")
        .with_icon(build_tray_icon()?)
        .with_menu_on_left_click(false)
        .with_menu(Box::new(menu))
        .build()
        .context("failed to build tray icon")?;

    // Opening the app bundle should immediately show the current auth/status dialog.
    if let Err(err) = open_app_dialog(&paths, &runtime) {
        eprintln!("initial dialog failed: {err:#}");
        let _ = ui::show_error(&format!("Operation failed:\n{err}"));
    }

    event_loop.run(move |event, _event_loop_target, control_flow| {
        *control_flow = ControlFlow::Wait;

        if matches!(event, Event::NewEvents(StartCause::Init)) {
            // No-op. This match keeps startup behavior explicit.
        }

        while let Ok(menu_event) = MenuEvent::receiver().try_recv() {
            if menu_event.id == close_item.id() {
                if let Err(err) = close_tray_and_service(&paths, &runtime) {
                    eprintln!("close failed: {err:#}");
                    let _ = ui::show_error(&format!("Could not close background service:\n{err}"));
                    continue;
                }
                *control_flow = ControlFlow::Exit;
                return;
            }

            if menu_event.id == open_item.id()
                && let Err(err) = open_app_dialog(&paths, &runtime)
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
                && let Err(err) = open_app_dialog(&paths, &runtime)
            {
                eprintln!("open dialog failed: {err:#}");
                let _ = ui::show_error(&format!("Operation failed:\n{err}"));
            }
        }
    });
}

fn close_tray_and_service(paths: &ClientPaths, runtime: &Runtime) -> Result<()> {
    if let Err(err) = runtime.block_on(send_close_alert(paths)) {
        eprintln!("warning: could not send close alert: {err:#}");
    }
    launch_agent::stop_agent(paths).context("failed to stop background service")
}

fn open_app_dialog(paths: &ClientPaths, runtime: &Runtime) -> Result<()> {
    let app_status = collect_status(paths)?;
    if app_status.logged_in {
        let email = app_status.email.as_deref().unwrap_or("<unknown>");
        let device_id = app_status.device_id.as_deref().unwrap_or("<unknown>");
        let action = if app_status.screenshot_permission == ScreenshotPermissionStatus::Missing {
            ui::prompt_permission_issue_action(
                email,
                device_id,
                app_status.screenshot_permission.as_str(),
                app_status.daemon_status_updated_at.as_deref(),
                app_status.daemon_last_error.as_deref(),
            )?
        } else {
            ui::prompt_logged_in_action(
                email,
                device_id,
                app_status.screenshot_permission.as_str(),
                app_status.daemon_status_updated_at.as_deref(),
                app_status.daemon_last_error.as_deref(),
            )?
        };

        match action {
            Some(ui::LoggedInAction::RestartDaemon) => {
                restart_daemon(paths)?;
                ui::show_info("Background service restarted.")?;
            }
            Some(ui::LoggedInAction::Logout) => {
                runtime.block_on(logout(paths))?;
                ui::show_info("Signed out. Monitoring disabled on this device.")?;
            }
            _ => {}
        }
        return Ok(());
    }

    let Some(input) = ui::prompt_login()? else {
        return Ok(());
    };

    let device_id = runtime.block_on(login(paths, &input.email, &input.password))?;
    ui::show_info(&format!("Signed in.\nDevice id: {device_id}"))?;
    Ok(())
}

fn restart_daemon(paths: &ClientPaths) -> Result<()> {
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    launch_agent::ensure_agent_running(paths, &exe).context("failed to restart background service")
}

async fn login(paths: &ClientPaths, email: &str, password: &str) -> Result<String> {
    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let auth_client = AuthClient::new(token_store.clone())?;
    let host = hostname::get()
        .ok()
        .and_then(|s| s.into_string().ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "mac-device".to_string());

    let api_client = ApiClient::new()?;
    let login_result = login_and_register_device(
        &auth_client,
        &api_client,
        token_store.as_ref(),
        LoginCommandInput {
            email,
            password,
            device_name: &host,
            platform: "macos",
        },
    )
    .await
    .context("login failed")?;

    let mut new_state = load_state(&paths.state_file)?;
    new_state.device_id = Some(login_result.device_id.clone());
    new_state.monitoring_enabled = true;
    new_state.email = Some(email.to_string());
    new_state.e2ee_user_id = Some(login_result.user_id);
    save_state(&paths.state_file, &new_state)?;

    if let Ok(exe) = std::env::current_exe() {
        launch_agent::ensure_agent_running(paths, &exe)?;
    }

    Ok(login_result.device_id)
}

async fn logout(paths: &ClientPaths) -> Result<()> {
    let mut state = load_state(&paths.state_file)?;
    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let auth_client = AuthClient::new(token_store.clone())?;
    let api_client = ApiClient::new()?;
    let metadata = vec![("source".to_string(), "mac_ui".to_string())];
    let _ = logout_and_clear_tokens_with_alert(
        &auth_client,
        Some(&api_client),
        token_store.as_ref(),
        state.device_id.as_deref(),
        &metadata,
    )
    .await;

    state.monitoring_enabled = false;
    state.device_id = None;
    state.email = None;
    state.e2ee_user_id = None;
    save_state(&paths.state_file, &state)?;
    Ok(())
}

async fn send_close_alert(paths: &ClientPaths) -> Result<()> {
    let state = load_state(&paths.state_file)?;
    let Some(device_id) = state.device_id.as_deref() else {
        return Ok(());
    };

    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let Some(access_token) = token_store.get_access_token()? else {
        return Ok(());
    };

    let api_client = ApiClient::new()?;
    let metadata = vec![
        ("source".to_string(), "mac_tray_menu".to_string()),
        ("reason".to_string(), "tray_close_requested".to_string()),
    ];

    let _ = api_client
        .create_alert_log(
            &access_token,
            device_id,
            "manual_override",
            &metadata,
            Utc::now(),
        )
        .await;
    Ok(())
}

fn status(paths: ClientPaths) -> Result<()> {
    let app_status = collect_status(&paths)?;

    println!("state: {}", app_status.state_label());
    println!("logged_in: {}", app_status.logged_in);
    println!("monitoring_enabled: {}", app_status.monitoring_enabled);
    println!("email: {}", app_status.email.as_deref().unwrap_or("<none>"));
    println!(
        "device_id: {}",
        app_status.device_id.as_deref().unwrap_or("<none>")
    );
    println!(
        "screenshot_permission: {}",
        app_status.screenshot_permission.as_str()
    );
    println!(
        "daemon_last_error: {}",
        app_status.daemon_last_error.as_deref().unwrap_or("<none>")
    );
    println!(
        "daemon_status_updated_at: {}",
        app_status
            .daemon_status_updated_at
            .as_deref()
            .unwrap_or("<none>")
    );
    println!(
        "capture_interval_seconds: {}",
        resolve_capture_interval_seconds()
    );
    println!("batch_window_seconds: {}", resolve_batch_window_seconds());
    println!("base_api_url: {}", resolve_base_api_url());
    println!("timestamp: {}", Utc::now().to_rfc3339());
    Ok(())
}

#[derive(Debug)]
struct AppStatus {
    logged_in: bool,
    monitoring_enabled: bool,
    email: Option<String>,
    device_id: Option<String>,
    screenshot_permission: ScreenshotPermissionStatus,
    daemon_last_error: Option<String>,
    daemon_status_updated_at: Option<String>,
}

impl AppStatus {
    fn state_label(&self) -> &'static str {
        if !self.logged_in {
            "logged_out"
        } else if self.screenshot_permission == ScreenshotPermissionStatus::Missing {
            "permissions_required"
        } else {
            "ok"
        }
    }
}

fn collect_status(paths: &ClientPaths) -> Result<AppStatus> {
    let state = load_state(&paths.state_file)?;
    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let logged_in = token_store.get_access_token()?.is_some() && state.device_id.is_some();
    let daemon_status = load_daemon_status(&paths.daemon_status_file)?;

    Ok(AppStatus {
        logged_in,
        monitoring_enabled: state.monitoring_enabled,
        email: state.email,
        device_id: state.device_id,
        screenshot_permission: daemon_status.screenshot_permission,
        daemon_last_error: daemon_status.last_error,
        daemon_status_updated_at: daemon_status.updated_at,
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
