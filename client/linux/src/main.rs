mod capture;
mod config;
mod daemon;
mod tray;

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use virtue_core::storage::FileStateStore;
use virtue_core::{AuthState, MonitorService, ServiceStatus};

use crate::capture::{CaptureBackend, LinuxPlatformHooks, detect_backend, probe_backend};
use crate::config::{ClientPaths, build_core_config};

const BUILD_LABEL: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Parser)]
#[command(name = "virtue")]
#[command(about = "Virtue Linux client")]
#[command(version = BUILD_LABEL)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Login {
        #[arg(long)]
        email: Option<String>,
    },
    Logout {
        #[arg(long)]
        yes: bool,
    },
    Daemon,
    Status,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = ClientPaths::discover()?;
    paths.ensure_dirs()?;

    match cli.command {
        Commands::Login { email } => login(paths, email),
        Commands::Logout { yes } => logout(paths, yes),
        Commands::Daemon => daemon::run_daemon(&paths).await,
        Commands::Status => status(paths),
    }
}

fn login(paths: ClientPaths, email: Option<String>) -> Result<()> {
    let email = match email {
        Some(email) => email,
        None => prompt_line("Email")?,
    };
    let password = rpassword::prompt_password("Password: ")?;

    let mut service = MonitorService::setup(build_core_config(&paths), LinuxPlatformHooks::new())?;
    let login_result = service.login(&email, &password).context("login failed")?;

    let probe = probe_backend();
    println!("{}", probe.guidance);
    println!(
        "Logged in. Device id: {}",
        login_result
            .device
            .as_ref()
            .map(|device| device.device_id.as_str())
            .unwrap_or("<unknown>")
    );
    if !probe.captured_ok {
        println!(
            "Capture is not yet working; service will run and log missed captures until fixed."
        );
    }

    Ok(())
}

fn logout(paths: ClientPaths, yes: bool) -> Result<()> {
    let store = FileStateStore::new(&paths.state_dir)?;
    let auth = store.load_auth_state()?;
    if auth.device_credentials.is_none() {
        println!("Already logged out.");
        return Ok(());
    }

    println!(
        "Warning: logging out will send a log event indicating monitoring was turned off on this device."
    );

    if !yes && !prompt_yes_no("Continue logout?", false)? {
        println!("Logout cancelled.");
        return Ok(());
    }

    let mut service = MonitorService::setup(build_core_config(&paths), LinuxPlatformHooks::new())?;
    service.logout()?;

    println!("Logged out. Monitoring is disabled on this device until you run `virtue login`.");
    Ok(())
}

fn status(paths: ClientPaths) -> Result<()> {
    let store = FileStateStore::new(&paths.state_dir)?;
    let auth = store.load_auth_state()?;
    let status = load_service_status(&store, &auth)?;
    let device_settings = store.load_device_settings()?;
    let mut config = build_core_config(&paths);
    config.refresh_from_runtime_file()?;

    println!("logged_in: {}", auth.device_credentials.is_some());
    println!("running: {}", status.is_running);
    println!("pending_request_count: {}", status.pending_request_count);
    println!(
        "device_id: {}",
        status.device_id.as_deref().unwrap_or("<none>")
    );
    println!(
        "device_enabled: {}",
        device_settings
            .as_ref()
            .map(|settings| settings.enabled.to_string())
            .unwrap_or_else(|| "<unknown>".to_string())
    );
    println!(
        "capture_interval_seconds: {}",
        config.screenshot_interval.as_secs()
    );
    println!("batch_window_seconds: {}", config.batch_interval.as_secs());
    println!("base_api_url: {}", config.api_base_url);
    println!(
        "backend: {}",
        match detect_backend() {
            Some(CaptureBackend::Wayland) => "wayland",
            Some(CaptureBackend::X11) => "x11",
            None => "<unknown>",
        }
    );

    Ok(())
}

fn prompt_line(label: &str) -> Result<String> {
    print!("{label}: ");
    io::stdout().flush().context("failed flushing stdout")?;

    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .context("failed reading stdin")?;
    Ok(value.trim().to_string())
}

fn prompt_yes_no(prompt: &str, default_yes: bool) -> Result<bool> {
    let suffix = if default_yes { "[Y/n]" } else { "[y/N]" };

    loop {
        print!("{prompt} {suffix} ");
        io::stdout().flush().context("failed flushing stdout")?;

        let mut value = String::new();
        io::stdin()
            .read_line(&mut value)
            .context("failed reading stdin")?;

        let normalized = value.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Ok(default_yes);
        }
        if matches!(normalized.as_str(), "y" | "yes") {
            return Ok(true);
        }
        if matches!(normalized.as_str(), "n" | "no") {
            return Ok(false);
        }

        println!("Please answer y or n.");
    }
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
