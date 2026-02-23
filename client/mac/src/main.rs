mod api;
mod capture;
mod config;
mod daemon;
mod launch_agent;
mod ui;

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use serde_json::json;
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tokio::runtime::Runtime;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use bepure_client_core::{AuthClient, FileTokenStore, TokenStore};

use crate::api::ApiClient;
use crate::config::{ClientPaths, load_state, save_state};

#[derive(Debug, Parser)]
#[command(name = "bepure-mac-client")]
#[command(about = "BePure macOS tray client")]
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
    let open_item = MenuItem::new("Open BePure", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    menu.append(&open_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit_item)?;

    let _tray_icon = TrayIconBuilder::new()
        .with_tooltip("BePure")
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
            if menu_event.id == quit_item.id() {
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

fn open_app_dialog(paths: &ClientPaths, runtime: &Runtime) -> Result<()> {
    if is_logged_in(paths)? {
        let state = load_state(&paths.state_file)?;
        let device_id = state.device_id.as_deref().unwrap_or("<unknown>");

        if ui::prompt_logged_in_action(device_id)?.unwrap_or(false) {
            runtime.block_on(logout(paths))?;
            ui::show_info("Signed out. Monitoring disabled on this device.")?;
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

async fn login(paths: &ClientPaths, email: &str, password: &str) -> Result<String> {
    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let auth_client = AuthClient::new(token_store.clone())?;

    auth_client
        .login(email, password)
        .await
        .context("login failed")?;

    let access_token = token_store
        .get_access_token()?
        .context("missing access token after login")?;

    let host = hostname::get()
        .ok()
        .and_then(|s| s.into_string().ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "mac-device".to_string());

    let state = load_state(&paths.state_file)?;
    let api_client = ApiClient::new()?;
    let registration = api_client
        .register_device(&access_token, &host, state.capture_interval_seconds.max(30))
        .await
        .context("device registration failed")?;

    let mut new_state = state;
    new_state.device_id = Some(registration.id.clone());
    new_state.monitoring_enabled = true;
    save_state(&paths.state_file, &new_state)?;

    if let Ok(exe) = std::env::current_exe() {
        launch_agent::ensure_agent_running(paths, &exe)?;
    }

    Ok(registration.id)
}

async fn logout(paths: &ClientPaths) -> Result<()> {
    let mut state = load_state(&paths.state_file)?;
    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let access_token = token_store.get_access_token()?;

    if let (Some(token), Some(device_id)) = (access_token.as_deref(), state.device_id.as_deref()) {
        let api_client = ApiClient::new()?;
        let mut metadata = BTreeMap::new();
        metadata.insert("reason".to_string(), json!("user_logout"));
        let _ = api_client
            .send_log(token, "manual_override", device_id, None, metadata)
            .await;
    }

    if access_token.is_some() {
        let auth_client = AuthClient::new(token_store.clone())?;
        let _ = auth_client.logout().await;
    }

    token_store.clear_access_token()?;
    state.monitoring_enabled = false;
    state.device_id = None;
    save_state(&paths.state_file, &state)?;
    Ok(())
}

fn is_logged_in(paths: &ClientPaths) -> Result<bool> {
    let state = load_state(&paths.state_file)?;
    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let logged_in = token_store.get_access_token()?.is_some() && state.device_id.is_some();
    Ok(logged_in)
}

fn status(paths: ClientPaths) -> Result<()> {
    let state = load_state(&paths.state_file)?;
    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let logged_in = token_store.get_access_token()?.is_some();

    println!("logged_in: {}", logged_in);
    println!("monitoring_enabled: {}", state.monitoring_enabled);
    println!(
        "device_id: {}",
        state.device_id.as_deref().unwrap_or("<none>")
    );
    println!("timestamp: {}", Utc::now().to_rfc3339());
    Ok(())
}

fn build_tray_icon() -> Result<Icon> {
    let width = 32;
    let height = 32;
    let mut rgba = vec![0u8; width * height * 4];

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 4;
            let dx = x as i32 - 16;
            let dy = y as i32 - 16;
            let dist_sq = dx * dx + dy * dy;

            if dist_sq <= 14 * 14 {
                rgba[idx] = 40;
                rgba[idx + 1] = 180;
                rgba[idx + 2] = 99;
                rgba[idx + 3] = 255;
            } else {
                rgba[idx] = 0;
                rgba[idx + 1] = 0;
                rgba[idx + 2] = 0;
                rgba[idx + 3] = 0;
            }
        }
    }

    Ok(Icon::from_rgba(rgba, width as u32, height as u32)?)
}
