mod capture;
mod config;
mod daemon;
mod tray;

use std::collections::HashMap;
use std::io::{self, Write};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use virtue_core::storage::FileStateStore;
use virtue_core::{AuthState, MonitorService, ServiceStatus};

use crate::capture::{CaptureBackend, LinuxPlatformHooks, probe_backend};
use crate::config::{
    BASE_API_URL_ENV_VAR, BATCH_WINDOW_SECONDS_ENV_VAR, CAPTURE_INTERVAL_SECONDS_ENV_VAR,
    CaptureBackendHint, ClientPaths, build_core_config, clamp_batch_window_seconds,
    clamp_capture_interval_seconds, load_state, resolve_base_api_url, resolve_batch_window_seconds,
    resolve_capture_interval_seconds, save_state,
};

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
    apply_service_env_defaults("virtue.service");

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

    let mut service = MonitorService::setup(
        build_core_config(&paths),
        LinuxPlatformHooks::new(paths.clone()),
    )?;
    let login_result = service.login(&email, &password).context("login failed")?;

    let probe = probe_backend(None);
    println!("{}", probe.guidance);

    let mut state = load_state(&paths.client_state_file)?;
    state.backend_hint = probe.backend.map(|backend| match backend {
        CaptureBackend::Wayland => CaptureBackendHint::Wayland,
        CaptureBackend::X11 => CaptureBackendHint::X11,
    });
    save_state(&paths.client_state_file, &state)?;

    if !is_user_service_active("virtue.service")
        && prompt_yes_no("Install and start the virtue systemd user service?", true)?
        && let Err(err) = ensure_user_service_running()
    {
        eprintln!(
            "could not auto-start user service: {err}\nrun: systemctl --user daemon-reload && systemctl --user enable --now virtue.service"
        );
    }

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

    let mut service = MonitorService::setup(
        build_core_config(&paths),
        LinuxPlatformHooks::new(paths.clone()),
    )?;
    service.logout()?;

    println!("Logged out. Monitoring is disabled on this device until you run `virtue login`.");
    Ok(())
}

fn status(paths: ClientPaths) -> Result<()> {
    let client_state = load_state(&paths.client_state_file)?;
    let service_env = load_service_env("virtue.service");
    let capture_interval_seconds =
        resolve_capture_interval_seconds_for_status(service_env.as_ref());
    let batch_window_seconds = resolve_batch_window_seconds_for_status(service_env.as_ref());
    let base_api_url = resolve_base_api_url_for_status(service_env.as_ref());
    let store = FileStateStore::new(&paths.state_dir)?;
    let auth = store.load_auth_state()?;
    let status = load_service_status(&store, &auth)?;
    let device_settings = store.load_device_settings()?;

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
    println!("capture_interval_seconds: {}", capture_interval_seconds);
    println!("batch_window_seconds: {}", batch_window_seconds);
    println!("base_api_url: {}", base_api_url);
    println!(
        "backend: {}",
        match client_state.backend_hint {
            Some(CaptureBackendHint::Wayland) => "wayland",
            Some(CaptureBackendHint::X11) => "x11",
            None => "<unknown>",
        }
    );

    Ok(())
}

fn load_service_env(service: &str) -> Option<HashMap<String, String>> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(["show", service, "--property=Environment", "--value"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let raw = String::from_utf8(output.stdout).ok()?;
    if raw.trim().is_empty() {
        return None;
    }

    let mut map = HashMap::new();
    for token in raw.split_whitespace() {
        if let Some((key, value)) = token.split_once('=') {
            map.insert(key.to_string(), value.to_string());
        }
    }
    Some(map)
}

fn apply_service_env_defaults(service: &str) {
    let Some(vars) = load_service_env(service) else {
        return;
    };
    for (key, value) in vars {
        if std::env::var_os(&key).is_none() {
            unsafe {
                std::env::set_var(key, value);
            }
        }
    }
}

fn resolve_base_api_url_for_status(service_env: Option<&HashMap<String, String>>) -> String {
    if let Ok(value) = std::env::var(BASE_API_URL_ENV_VAR) {
        let normalized = value.trim().trim_end_matches('/').to_string();
        if !normalized.is_empty() {
            return normalized;
        }
    }
    if let Some(value) = service_env
        .and_then(|vars| vars.get(BASE_API_URL_ENV_VAR))
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
    {
        return value;
    }
    resolve_base_api_url()
}

fn resolve_capture_interval_seconds_for_status(
    service_env: Option<&HashMap<String, String>>,
) -> u64 {
    if std::env::var(CAPTURE_INTERVAL_SECONDS_ENV_VAR).is_ok() {
        return resolve_capture_interval_seconds();
    }
    service_env
        .and_then(|vars| vars.get(CAPTURE_INTERVAL_SECONDS_ENV_VAR))
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(clamp_capture_interval_seconds)
        .unwrap_or_else(resolve_capture_interval_seconds)
}

fn resolve_batch_window_seconds_for_status(service_env: Option<&HashMap<String, String>>) -> u64 {
    if std::env::var(BATCH_WINDOW_SECONDS_ENV_VAR).is_ok() {
        return resolve_batch_window_seconds();
    }
    service_env
        .and_then(|vars| vars.get(BATCH_WINDOW_SECONDS_ENV_VAR))
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(clamp_batch_window_seconds)
        .unwrap_or_else(resolve_batch_window_seconds)
}

fn ensure_user_service_running() -> Result<()> {
    run_systemctl_user(&[
        "import-environment",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "XDG_SESSION_TYPE",
        "DBUS_SESSION_BUS_ADDRESS",
    ])?;
    install_user_service_file()?;
    run_systemctl_user(&["daemon-reload"])?;
    run_systemctl_user(&["enable", "--now", "virtue.service"])?;
    Ok(())
}

fn install_user_service_file() -> Result<()> {
    let exe = std::env::current_exe().context("could not determine current executable path")?;
    let exe_str = exe.to_str().context("executable path is not valid UTF-8")?;

    let service_content =
        include_str!("../packaging/systemd/virtue.service").replace("/usr/bin/virtue", exe_str);

    let systemd_dir = dirs::config_dir()
        .context("could not determine config directory")?
        .join("systemd/user");

    std::fs::create_dir_all(&systemd_dir)
        .with_context(|| format!("could not create {}", systemd_dir.display()))?;

    let dest = systemd_dir.join("virtue.service");
    std::fs::write(&dest, service_content)
        .with_context(|| format!("could not write {}", dest.display()))?;

    Ok(())
}

fn run_systemctl_user(args: &[&str]) -> Result<()> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .with_context(|| format!("failed to run systemctl --user {}", args.join(" ")))?;

    if !status.success() {
        return Err(anyhow::anyhow!(
            "systemctl --user {} exited with {}",
            args.join(" "),
            status
        ));
    }

    Ok(())
}

fn is_user_service_active(service: &str) -> bool {
    Command::new("systemctl")
        .arg("--user")
        .args(["is-active", "--quiet", service])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
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
