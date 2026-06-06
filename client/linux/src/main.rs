mod capture;
mod config;
mod daemon;
mod tray;

use std::io::{self, Write};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::process::Command;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use virtue_core::events::UploadKind;
use virtue_core::{ControllerClient, MonitorService, ServiceStatus};

use crate::capture::{CaptureBackend, LinuxPlatformHooks, detect_backend, probe_backend};
use crate::config::{ClientPaths, build_core_config};

const BUILD_LABEL: &str = virtue_core::BUILD_LABEL;

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
    #[command(about = "Log in and register this Linux device")]
    Login {
        #[arg(long)]
        email: Option<String>,
    },
    #[command(about = "Log out and disable monitoring on this device")]
    Logout {
        #[arg(long)]
        yes: bool,
    },
    #[command(about = "Run or control the background monitoring daemon")]
    Daemon {
        #[command(subcommand)]
        command: Option<DaemonCommands>,
    },
    #[command(about = "Show current auth, capture, and upload status")]
    Status {
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Developer-only commands for test logs and batch uploads")]
    Dev {
        #[command(subcommand)]
        command: DevCommands,
    },
}

#[derive(Debug, Subcommand)]
enum DaemonCommands {
    #[command(about = "Start the user service via systemd")]
    Start,
    #[command(about = "Stop the user service via systemd and mark it as user-requested")]
    Stop {
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum DevCommands {
    #[command(about = "Upload a developer log immediately")]
    UploadLog(DeveloperEventArgs),
    #[command(about = "Queue a developer log into the next encrypted batch")]
    AddLog(DeveloperEventArgs),
    #[command(about = "Capture a screenshot and queue it into the next encrypted batch")]
    AddScreenshot(DeveloperEventArgs),
    #[command(about = "Upload any queued batch items right now")]
    UploadBatch,
}

#[derive(Debug, Args)]
struct DeveloperEventArgs {
    #[arg(long, default_value_t = 0.5_f32, value_parser = parse_risk)]
    risk: f32,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    details: Option<String>,
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
        Commands::Login { email } => tokio::task::block_in_place(|| login(paths, email)),
        Commands::Logout { yes } => tokio::task::block_in_place(|| logout(paths, yes)),
        Commands::Daemon { command } => daemon_command(paths, command).await,
        Commands::Status { json } => status(paths, json),
        Commands::Dev { command } => tokio::task::block_in_place(|| dev(paths, command)),
    }
}

async fn daemon_command(paths: ClientPaths, command: Option<DaemonCommands>) -> Result<()> {
    match command {
        None => daemon::run_daemon(&paths).await,
        Some(DaemonCommands::Start) => daemon_start(),
        Some(DaemonCommands::Stop { yes }) => {
            tokio::task::block_in_place(|| daemon_stop(paths, yes))
        }
    }
}

fn login(paths: ClientPaths, email: Option<String>) -> Result<()> {
    let email = match email {
        Some(email) => email,
        None => {
            let mut rl = rustyline::DefaultEditor::new()?;
            rl.readline("Email: ")?
        }
    };
    let password = prompt_password("Password: ")?;

    let sock = paths.state_dir.join("daemon.sock");
    let mut client =
        ControllerClient::connect(&sock).context("failed to connect to daemon (is it running?)")?;
    let device_id = client.login(&email, &password).context("login failed")?;

    let probe = probe_backend();
    println!("{}", probe.guidance);
    println!("Logged in. Device id: {device_id}");
    if !probe.captured_ok {
        println!(
            "Capture is not yet working; service will run and log missed captures until fixed."
        );
    }

    Ok(())
}

fn logout(paths: ClientPaths, yes: bool) -> Result<()> {
    println!(
        "Warning: logging out will alert people monitoring you and will recreate a new device on login."
    );

    if !yes && !prompt_yes_no("Continue logout?", false)? {
        println!("Logout cancelled.");
        return Ok(());
    }

    let sock = paths.state_dir.join("daemon.sock");
    let mut client =
        ControllerClient::connect(&sock).context("failed to connect to daemon (is it running?)")?;
    client.logout().context("logout failed")?;

    println!("Logged out. Monitoring is disabled on this device until you run `virtue login`.");
    Ok(())
}

fn status(paths: ClientPaths, json: bool) -> Result<()> {
    let mut config = build_core_config(&paths);
    config.refresh_from_runtime_file()?;
    let status = load_service_status(&paths)?;

    let logged_in = status.is_authenticated;
    let device_id = status.device_id.as_deref().unwrap_or("<none>").to_string();
    let backend = match detect_backend() {
        Some(CaptureBackend::Wayland) => "wayland",
        Some(CaptureBackend::X11) => "x11",
        None => "<unknown>",
    };

    if json {
        println!(
            "{}",
            serde_json::json!({
                "logged_in": logged_in,
                "running": status.is_running,
                "pending_request_count": status.pending_request_count,
                "device_id": device_id,
                "capture_interval_seconds": config.screenshot_interval.as_secs(),
                "batch_window_seconds": config.batch_interval.as_secs(),
                "base_api_url": config.api_base_url,
                "backend": backend,
            })
        );
    } else {
        println!("logged_in: {logged_in}");
        println!("running: {}", status.is_running);
        println!("pending_request_count: {}", status.pending_request_count);
        println!("device_id: {device_id}");
        println!(
            "capture_interval_seconds: {}",
            config.screenshot_interval.as_secs()
        );
        println!("batch_window_seconds: {}", config.batch_interval.as_secs());
        println!("base_api_url: {}", config.api_base_url);
        println!("backend: {backend}");
    }

    Ok(())
}

fn daemon_start() -> Result<()> {
    run_systemctl_user(["start", "virtue.service"])?;
    println!("Started virtue.service.");
    Ok(())
}

fn daemon_stop(paths: ClientPaths, yes: bool) -> Result<()> {
    if !is_user_service_active()? {
        println!("virtue.service is already stopped.");
        return Ok(());
    }

    println!("Warning: stopping the daemon will alert people monitoring you.");

    if !yes && !prompt_yes_no("Continue stopping the daemon?", false)? {
        println!("Daemon stop cancelled.");
        return Ok(());
    }

    let sock = paths.state_dir.join("daemon.sock");
    let mut client =
        ControllerClient::connect(&sock).context("failed to connect to daemon (is it running?)")?;
    client
        .request_user_stop("cli_daemon_stop")
        .context("failed to record stop intent")?;

    run_systemctl_user(["stop", "virtue.service"])?;

    println!("Stopped virtue.service.");
    Ok(())
}

fn is_user_service_active() -> Result<bool> {
    let status = Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "virtue.service"])
        .status()
        .context("failed to query virtue.service status with systemctl --user")?;

    match status.code() {
        Some(0) => Ok(true),
        Some(3) => Ok(false),
        _ => Err(anyhow::anyhow!(
            "systemctl --user is-active --quiet virtue.service exited with status {}",
            status
                .code()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "<signal>".to_string())
        )),
    }
}

fn run_systemctl_user<const N: usize>(args: [&str; N]) -> Result<()> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .with_context(|| format!("failed to run `systemctl --user {}`", args.join(" ")))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let details = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "no diagnostic output".to_string()
    };

    Err(anyhow::anyhow!(
        "`systemctl --user {}` failed: {}",
        args.join(" "),
        details
    ))
}

fn dev(paths: ClientPaths, command: DevCommands) -> Result<()> {
    match command {
        DevCommands::UploadLog(args) => dev_upload_log(paths, args),
        DevCommands::AddLog(args) => dev_add_log(paths, args),
        DevCommands::AddScreenshot(args) => dev_add_screenshot(paths, args),
        DevCommands::UploadBatch => dev_upload_batch(paths),
    }
}

fn dev_upload_log(paths: ClientPaths, args: DeveloperEventArgs) -> Result<()> {
    let title = args
        .title
        .unwrap_or_else(|| "Developer CLI log".to_string());
    let mut service = MonitorService::setup(build_core_config(&paths), LinuxPlatformHooks::new())?;
    // Use risk >= 1.0 so this always routes through the immediate (POST /log) path.
    service.send_log(
        1.0_f32,
        UploadKind::Dev {
            title,
            details: args.details,
        },
    )?;

    println!(
        "Recorded immediate developer log with risk {}.",
        format_risk(args.risk)
    );
    Ok(())
}

fn dev_add_log(paths: ClientPaths, args: DeveloperEventArgs) -> Result<()> {
    let title = args
        .title
        .unwrap_or_else(|| "Developer CLI batched log".to_string());
    let mut service = MonitorService::setup(build_core_config(&paths), LinuxPlatformHooks::new())?;
    service.send_log(
        args.risk,
        UploadKind::Dev {
            title,
            details: args.details,
        },
    )?;

    println!(
        "Queued developer log in the next batch with risk {}.",
        format_risk(args.risk)
    );
    Ok(())
}

fn dev_add_screenshot(paths: ClientPaths, args: DeveloperEventArgs) -> Result<()> {
    let mut service = MonitorService::setup(build_core_config(&paths), LinuxPlatformHooks::new())?;
    service.capture_batch_screenshot(Some(args.risk))?;

    println!(
        "Captured and queued a developer screenshot with risk {}.",
        format_risk(args.risk)
    );
    Ok(())
}

fn dev_upload_batch(paths: ClientPaths) -> Result<()> {
    let mut service = MonitorService::setup(build_core_config(&paths), LinuxPlatformHooks::new())?;
    let (attempted, remaining) = service.upload_pending_batch_now()?;

    if attempted == 0 {
        println!("No pending batch items to upload.");
        return Ok(());
    }

    if remaining == 0 {
        println!("Processed {attempted} batch item(s); no batch items remain queued.");
    } else {
        println!("Processed {attempted} batch item(s); {remaining} batch item(s) remain queued.");
    }

    Ok(())
}

fn prompt_password(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush().context("failed flushing stdout")?;

    enable_raw_mode().context("failed enabling raw terminal mode")?;
    let mut password = String::new();

    let result = (|| {
        loop {
            match crossterm::event::read().context("failed reading terminal event")? {
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    ..
                }) => break,
                Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers,
                    ..
                }) if modifiers.contains(KeyModifiers::CONTROL) => {
                    return Err(anyhow::anyhow!("interrupted"));
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char('d'),
                    modifiers,
                    ..
                }) if modifiers.contains(KeyModifiers::CONTROL) => {
                    return Err(anyhow::anyhow!("interrupted"));
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Backspace,
                    ..
                }) if !password.is_empty() => {
                    password.pop();
                    print!("\x08 \x08");
                    io::stdout().flush().context("failed flushing stdout")?;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char(c),
                    modifiers,
                    ..
                }) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    password.push(c);
                    print!("*");
                    io::stdout().flush().context("failed flushing stdout")?;
                }
                _ => {}
            }
        }
        Ok(password)
    })();

    disable_raw_mode().context("failed disabling raw terminal mode")?;
    println!();
    result
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

fn parse_risk(raw: &str) -> std::result::Result<f32, String> {
    let risk = raw
        .parse::<f32>()
        .map_err(|_| "risk must be a number between 0 and 1".to_string())?;
    if !risk.is_finite() || !(0.0..=1.0).contains(&risk) {
        return Err("risk must be a number between 0 and 1".to_string());
    }
    Ok(risk)
}

fn format_risk(risk: f32) -> String {
    let mut value = format!("{risk:.3}");
    while value.contains('.') && value.ends_with('0') {
        value.pop();
    }
    if value.ends_with('.') {
        value.pop();
    }
    value
}

fn load_service_status(paths: &ClientPaths) -> Result<ServiceStatus> {
    // Try to get live status from the daemon via IPC; fall back to defaults.
    let sock = paths.state_dir.join("daemon.sock");
    if let Ok(mut client) = ControllerClient::connect(&sock)
        && let Ok(status) = client.get_status()
    {
        return Ok(status);
    }
    Ok(ServiceStatus::default())
}

#[cfg(test)]
mod tests {
    use super::{Cli, Commands, DaemonCommands};
    use clap::Parser;

    #[test]
    fn cli_accepts_login_command() {
        let cli = Cli::try_parse_from(["virtue", "login"]).expect("login command should parse");
        assert!(matches!(cli.command, Commands::Login { email: None }));
    }

    #[test]
    fn cli_accepts_logout_command() {
        let cli = Cli::try_parse_from(["virtue", "logout"]).expect("logout command should parse");
        assert!(matches!(cli.command, Commands::Logout { yes: false }));
    }

    #[test]
    fn cli_accepts_status_command() {
        let cli = Cli::try_parse_from(["virtue", "status"]).expect("status command should parse");
        assert!(matches!(cli.command, Commands::Status { json: false }));
    }

    #[test]
    fn cli_accepts_status_json_flag() {
        let cli = Cli::try_parse_from(["virtue", "status", "--json"])
            .expect("status --json should parse");
        assert!(matches!(cli.command, Commands::Status { json: true }));
    }

    #[test]
    fn cli_accepts_bare_daemon_command() {
        let cli = Cli::try_parse_from(["virtue", "daemon"]).expect("daemon command should parse");

        match cli.command {
            Commands::Daemon { command } => assert!(command.is_none()),
            other => panic!("expected daemon command, got {other:?}"),
        }
    }

    #[test]
    fn cli_accepts_daemon_start_subcommand() {
        let cli =
            Cli::try_parse_from(["virtue", "daemon", "start"]).expect("daemon start should parse");

        match cli.command {
            Commands::Daemon { command } => {
                assert!(matches!(command, Some(DaemonCommands::Start)));
            }
            other => panic!("expected daemon command, got {other:?}"),
        }
    }

    #[test]
    fn cli_accepts_daemon_stop_subcommand() {
        let cli =
            Cli::try_parse_from(["virtue", "daemon", "stop"]).expect("daemon stop should parse");

        match cli.command {
            Commands::Daemon { command } => {
                assert!(matches!(command, Some(DaemonCommands::Stop { yes: false })));
            }
            other => panic!("expected daemon command, got {other:?}"),
        }
    }

    #[test]
    fn cli_accepts_daemon_stop_yes_flag() {
        let cli = Cli::try_parse_from(["virtue", "daemon", "stop", "--yes"])
            .expect("daemon stop --yes should parse");

        match cli.command {
            Commands::Daemon { command } => {
                assert!(matches!(command, Some(DaemonCommands::Stop { yes: true })));
            }
            other => panic!("expected daemon command, got {other:?}"),
        }
    }
}
