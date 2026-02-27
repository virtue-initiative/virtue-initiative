mod api;
mod capture;
mod config;
mod daemon;

use std::io::{self, Write};
use std::process::{Command, ExitCode};
use std::sync::Arc;

use anyhow::{Context, Result};
use base64::Engine;
use clap::{Parser, Subcommand};
use serde::Deserialize;

use bepure_client_core::{
    AuthClient, FileTokenStore, TokenStore, apply_dev_env, derive_key, resolve_capture_interval_seconds,
};

use crate::api::ApiClient;
use crate::capture::{CaptureBackend, probe_backend};
use crate::config::{CaptureBackendHint, ClientPaths, load_state, save_state};

#[derive(Debug, Parser)]
#[command(name = "bepure")]
#[command(about = "BePure Linux client")]
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
    /// Set how many seconds of captures to accumulate before uploading a batch.
    SetBatchWindow {
        seconds: u64,
    },
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

    match cli.command {
        Commands::Login { email } => login(paths, email).await,
        Commands::Logout { yes } => logout(paths, yes).await,
        Commands::Daemon => daemon::run_daemon(&paths).await,
        Commands::Status => status(paths),
        Commands::SetBatchWindow { seconds } => set_batch_window(paths, seconds),
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

    // Derive and store the E2EE key.
    let user_id = parse_jwt_sub(&access_token)
        .context("could not extract user ID from access token")?;
    let e2ee_password = rpassword::prompt_password("E2EE encryption password: ")?;
    let e2ee_key = derive_key(&e2ee_password, &user_id);
    token_store.set_e2ee_key(&e2ee_key)?;

    let probe = probe_backend(None);
    println!("{}", probe.guidance);

    let api_client = ApiClient::new()?;
    let mut state = load_state(&paths.state_file)?;
    let capture_interval_seconds = resolve_capture_interval_seconds(state.capture_interval_seconds);

    let host = hostname::get()
        .ok()
        .and_then(|s| s.into_string().ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "linux-device".to_string());

    let registration = api_client
        .register_device(&access_token, &host, capture_interval_seconds)
        .await
        .context("device registration failed")?;

    state.device_id = Some(registration.id.clone());
    state.monitoring_enabled = true;
    state.e2ee_user_id = Some(user_id.clone());
    state.backend_hint = probe.backend.map(|backend| match backend {
        CaptureBackend::Wayland => CaptureBackendHint::Wayland,
        CaptureBackend::X11 => CaptureBackendHint::X11,
    });
    save_state(&paths.state_file, &state)?;

    if prompt_yes_no("Install and start the bepure systemd user service?")? {
        if let Err(err) = ensure_user_service_running() {
            eprintln!(
                "could not auto-start user service: {err}\nrun: systemctl --user daemon-reload && systemctl --user enable --now bepure.service"
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

    if !yes && !prompt_yes_no("Continue logout? [y/N]")? {
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
    state.monitoring_enabled = false;
    state.device_id = None;
    state.e2ee_user_id = None;
    save_state(&paths.state_file, &state)?;

    println!("Logged out. Monitoring is disabled on this device until you run `bepure login`.");
    Ok(())
}

fn status(paths: ClientPaths) -> Result<()> {
    let state = load_state(&paths.state_file)?;
    let effective_interval_seconds =
        resolve_capture_interval_seconds(state.capture_interval_seconds);
    let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
    let logged_in = token_store.get_access_token()?.is_some();
    let refresh_token_present = token_store.get_refresh_token()?.is_some();

    println!("logged_in: {}", logged_in);
    println!("refresh_token_present: {}", refresh_token_present);
    println!("monitoring_enabled: {}", state.monitoring_enabled);
    println!(
        "device_id: {}",
        state.device_id.as_deref().unwrap_or("<none>")
    );
    println!(
        "capture_interval_seconds: {} (effective: {})",
        state.capture_interval_seconds, effective_interval_seconds
    );
    println!("batch_window_seconds: {}", state.batch_window_seconds);
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

fn set_batch_window(paths: ClientPaths, seconds: u64) -> Result<()> {
    let mut state = load_state(&paths.state_file)?;
    state.batch_window_seconds = seconds;
    save_state(&paths.state_file, &state)?;
    println!("batch_window_seconds set to {seconds}");
    Ok(())
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
    run_systemctl_user(&["enable", "--now", "bepure.service"])?;
    Ok(())
}

fn install_user_service_file() -> Result<()> {
    let exe = std::env::current_exe().context("could not determine current executable path")?;
    let exe_str = exe.to_str().context("executable path is not valid UTF-8")?;

    let service_content =
        include_str!("../packaging/systemd/bepure.service").replace("/usr/bin/bepure", exe_str);

    let systemd_dir = dirs::config_dir()
        .context("could not determine config directory")?
        .join("systemd/user");

    std::fs::create_dir_all(&systemd_dir)
        .with_context(|| format!("could not create {}", systemd_dir.display()))?;

    let dest = systemd_dir.join("bepure.service");
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

fn prompt_line(label: &str) -> Result<String> {
    print!("{label}: ");
    io::stdout().flush().context("failed flushing stdout")?;

    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .context("failed reading stdin")?;
    Ok(value.trim().to_string())
}

fn prompt_yes_no(prompt: &str) -> Result<bool> {
    print!("{prompt} ");
    io::stdout().flush().context("failed flushing stdout")?;

    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .context("failed reading stdin")?;

    let normalized = value.trim().to_ascii_lowercase();
    Ok(matches!(normalized.as_str(), "y" | "yes"))
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
