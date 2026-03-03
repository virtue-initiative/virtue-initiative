mod api;
mod capture;
mod config;
mod daemon;
mod tray;

use std::io::{self, Write};
use std::process::{Command, ExitCode};
use std::sync::Arc;

use anyhow::{Context, Result};
use base64::Engine;
use clap::{Parser, Subcommand};
use serde::Deserialize;

use virtue_client_core::{
    AuthClient, BASE_API_URL_ENV_VAR, BATCH_WINDOW_SECONDS_ENV_VAR,
    CAPTURE_INTERVAL_SECONDS_ENV_VAR, FileTokenStore, TokenStore, apply_dev_env,
    apply_env_defaults_from_map,
    clamp_batch_window_seconds, clamp_capture_interval_seconds, resolve_base_api_url,
    resolve_batch_window_seconds, resolve_capture_interval_seconds,
};

use crate::api::ApiClient;
use crate::capture::{CaptureBackend, probe_backend};
use crate::config::{CaptureBackendHint, ClientPaths, load_state, save_state};

#[derive(Debug, Parser)]
#[command(name = "virtue")]
#[command(about = "Virtue Linux client")]
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
    apply_dev_env();

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
        Commands::Login { email } => login(paths, email).await,
        Commands::Logout { yes } => logout(paths, yes).await,
        Commands::Daemon => daemon::run_daemon(&paths).await,
        Commands::Status => status(paths),
    }
}

async fn login(paths: ClientPaths, email: Option<String>) -> Result<()> {
    let email = match email {
        Some(email) => email,
        None => prompt_line("Email")?,
    };
    let password = rpassword::prompt_password("Password: ")?;

    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let auth_client = AuthClient::new(token_store.clone())?;

    auth_client
        .login(&email, &password)
        .await
        .context("login failed")?;

    let access_token = token_store
        .get_access_token()?
        .context("missing access token after login")?;

    // Store the wrapping key (derived from login password) and fetch the E2EE key from server.
    let user_id =
        parse_jwt_sub(&access_token).context("could not extract user ID from access token")?;
    auth_client
        .store_wrapping_key(&password, &user_id)
        .context("could not store wrapping key")?;
    auth_client
        .fetch_and_decrypt_e2ee_key(&access_token)
        .await
        .context("could not retrieve E2EE key from server")?;

    let probe = probe_backend(None);
    println!("{}", probe.guidance);

    let api_client = ApiClient::new()?;
    let mut state = load_state(&paths.state_file)?;

    let host = hostname::get()
        .ok()
        .and_then(|s| s.into_string().ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "linux-device".to_string());

    let registration = api_client
        .register_device(&access_token, &host)
        .await
        .context("device registration failed")?;

    state.device_id = Some(registration.id.clone());
    state.monitoring_enabled = true;
    state.email = Some(email.clone());
    state.e2ee_user_id = Some(user_id.clone());
    state.backend_hint = probe.backend.map(|backend| match backend {
        CaptureBackend::Wayland => CaptureBackendHint::Wayland,
        CaptureBackend::X11 => CaptureBackendHint::X11,
    });
    save_state(&paths.state_file, &state)?;

    if !is_user_service_active("virtue.service")
        && prompt_yes_no("Install and start the virtue systemd user service?", true)?
    {
        if let Err(err) = ensure_user_service_running() {
            eprintln!(
                "could not auto-start user service: {err}\nrun: systemctl --user daemon-reload && systemctl --user enable --now virtue.service"
            );
        }
    }

    println!("Logged in. Device id: {}", registration.id);
    if !probe.captured_ok {
        println!(
            "Capture is not yet working; service will run and log missed captures until fixed."
        );
    }

    Ok(())
}

async fn logout(paths: ClientPaths, yes: bool) -> Result<()> {
    let mut state = load_state(&paths.state_file)?;
    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));

    let access_token = token_store.get_access_token()?;
    if access_token.is_none() && state.device_id.is_none() {
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

    if let (Some(token), Some(_device_id)) = (access_token.as_deref(), state.device_id.as_deref()) {
        let auth_client2 = AuthClient::new(token_store.clone())?;
        let _ = auth_client2.logout().await;
        let _ = token;
    }

    token_store.clear_access_token()?;
    token_store.clear_refresh_token()?;
    token_store.clear_e2ee_key()?;
    token_store.clear_wrapping_key()?;
    state.monitoring_enabled = false;
    state.email = None;
    state.device_id = None;
    state.e2ee_user_id = None;
    save_state(&paths.state_file, &state)?;

    println!("Logged out. Monitoring is disabled on this device until you run `virtue login`.");
    Ok(())
}

fn status(paths: ClientPaths) -> Result<()> {
    let state = load_state(&paths.state_file)?;
    let service_env = load_service_env("virtue.service");
    let capture_interval_seconds =
        resolve_capture_interval_seconds_for_status(service_env.as_ref());
    let batch_window_seconds = resolve_batch_window_seconds_for_status(service_env.as_ref());
    let base_api_url = resolve_base_api_url_for_status(service_env.as_ref());
    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let logged_in = token_store.get_access_token()?.is_some();
    let refresh_token_present = token_store.get_refresh_token()?.is_some();

    println!("logged_in: {}", logged_in);
    println!("refresh_token_present: {}", refresh_token_present);
    println!("monitoring_enabled: {}", state.monitoring_enabled);
    println!("email: {}", state.email.as_deref().unwrap_or("<none>"));
    println!(
        "device_id: {}",
        state.device_id.as_deref().unwrap_or("<none>")
    );
    println!("capture_interval_seconds: {}", capture_interval_seconds);
    println!("batch_window_seconds: {}", batch_window_seconds);
    println!("base_api_url: {}", base_api_url);
    println!(
        "backend: {}",
        match state.backend_hint {
            Some(CaptureBackendHint::Wayland) => "wayland",
            Some(CaptureBackendHint::X11) => "x11",
            None => "<unknown>",
        }
    );

    Ok(())
}

fn load_service_env(service: &str) -> Option<std::collections::HashMap<String, String>> {
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

    let mut map = std::collections::HashMap::new();
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
    apply_env_defaults_from_map(&vars);
}

fn resolve_base_api_url_for_status(
    service_env: Option<&std::collections::HashMap<String, String>>,
) -> String {
    if let Ok(value) = std::env::var(BASE_API_URL_ENV_VAR) {
        let normalized = value.trim().trim_end_matches('/').to_string();
        if !normalized.is_empty() {
            return normalized;
        }
    }
    if let Some(value) = service_env
        .and_then(|vars| vars.get(BASE_API_URL_ENV_VAR))
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
    {
        return value;
    }
    resolve_base_api_url()
}

fn resolve_capture_interval_seconds_for_status(
    service_env: Option<&std::collections::HashMap<String, String>>,
) -> u64 {
    if std::env::var(CAPTURE_INTERVAL_SECONDS_ENV_VAR).is_ok() {
        return resolve_capture_interval_seconds();
    }
    service_env
        .and_then(|vars| vars.get(CAPTURE_INTERVAL_SECONDS_ENV_VAR))
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(clamp_capture_interval_seconds)
        .unwrap_or_else(resolve_capture_interval_seconds)
}

fn resolve_batch_window_seconds_for_status(
    service_env: Option<&std::collections::HashMap<String, String>>,
) -> u64 {
    if std::env::var(BATCH_WINDOW_SECONDS_ENV_VAR).is_ok() {
        return resolve_batch_window_seconds();
    }
    service_env
        .and_then(|vars| vars.get(BATCH_WINDOW_SECONDS_ENV_VAR))
        .and_then(|v| v.trim().parse::<u64>().ok())
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

#[derive(Deserialize)]
struct JwtClaims {
    sub: Option<String>,
}

/// Extract the `sub` claim (user ID) from a JWT without verifying the signature.
fn parse_jwt_sub(token: &str) -> Option<String> {
    let payload_segment = token.split('.').nth(1)?;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_segment)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload_segment))
        .ok()?;
    let claims: JwtClaims = serde_json::from_slice(&payload).ok()?;
    claims.sub.filter(|s| !s.is_empty())
}
