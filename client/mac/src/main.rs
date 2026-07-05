use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use virtue_core::{AuthState, ClientController, ServiceStatus};

use virtue_mac::config::{ClientPaths, build_core_config, read_auth_state};
use virtue_mac::daemon;
use virtue_mac::runtime_env::apply_runtime_env;

const BUILD_LABEL: &str = virtue_core::BUILD_LABEL;

#[derive(Debug, Parser)]
#[command(name = "virtue-mac")]
#[command(about = "Virtue macOS background monitoring daemon")]
#[command(version = BUILD_LABEL)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run the background monitoring service (invoked by launchd).
    Daemon,
    /// Print the current service status as plain text.
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
        Commands::Daemon => daemon::run_daemon(&paths),
        Commands::Status => status(&paths),
    }
}

fn status(paths: &ClientPaths) -> Result<()> {
    println!("{}", render_status_text(paths)?);
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
