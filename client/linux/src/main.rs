mod capture;
mod config;
mod daemon;
mod tray;

use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::process::Command;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
#[cfg(debug_assertions)]
use virtue_core::{AlertReason, ScreenshotSkipReason};
use virtue_core::{ClientController, ScreenshotHooks, ServiceStatus, Upload, UploadKind};

use crate::capture::{CaptureBackend, LinuxPlatformHooks, detect_backend, probe_backend};
use crate::config::{ClientPaths, build_core_config, default_device_name};

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
        #[arg(long)]
        device_name: Option<String>,
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
    #[cfg(debug_assertions)]
    #[command(about = "Queue a log of any type into the next batch (debug builds only)")]
    Send(SendLogArgs),
}

/// Args for `dev send` — lets developers emit any log type to exercise the
/// web app's icons/titles. Debug builds only.
#[cfg(debug_assertions)]
#[derive(Debug, Args)]
struct SendLogArgs {
    /// Log type to emit. Use `all` to queue one of every type.
    #[arg(long = "type", value_name = "TYPE")]
    log_type: String,
    /// Alert reason (snake_case) when --type lifecycle_alert, e.g. late_wakeup.
    #[arg(long)]
    reason: Option<String>,
    /// Message body when --type alert.
    #[arg(long)]
    message: Option<String>,
    /// Title when --type dev.
    #[arg(long)]
    title: Option<String>,
    /// Details when --type dev.
    #[arg(long)]
    details: Option<String>,
    #[arg(long, default_value_t = 0.5_f32, value_parser = parse_risk)]
    risk: f32,
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
        Commands::Login { email, device_name } => {
            tokio::task::block_in_place(|| login(paths, email, device_name))
        }
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

fn login(paths: ClientPaths, email: Option<String>, device_name: Option<String>) -> Result<()> {
    let email = match email {
        Some(email) => email,
        None => {
            let mut rl = rustyline::DefaultEditor::new()?;
            rl.readline("Email: ")?
        }
    };
    let password = prompt_password("Password: ")?;

    // Resolve the device name: use the flag if given, otherwise prompt
    // interactively with the hostname as the blank-accepts default.
    let default_name = default_device_name();
    let device_name = match device_name {
        Some(name) => name,
        None => {
            let mut rl = rustyline::DefaultEditor::new()?;
            let entered = rl.readline(&format!("Device name [{default_name}]: "))?;
            let entered = entered.trim();
            if entered.is_empty() {
                default_name
            } else {
                entered.to_string()
            }
        }
    };

    let sock = paths.state_dir.join("daemon.sock");
    let mut client =
        ClientController::connect(&sock).context("failed to connect to daemon (is it running?)")?;
    let device_id = client
        .login(&email, &password, Some(&device_name))
        .context("login failed")?;

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
        ClientController::connect(&sock).context("failed to connect to daemon (is it running?)")?;
    client.logout().context("logout failed")?;

    println!("Logged out. Monitoring is disabled on this device until you run `virtue login`.");
    Ok(())
}

fn status(paths: ClientPaths, json: bool) -> Result<()> {
    let config = build_core_config(&paths);
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

fn service_name() -> String {
    match config::INSTANCE {
        Some(n) if !n.is_empty() => format!("virtue-{n}.service"),
        _ => "virtue.service".to_string(),
    }
}

fn daemon_start() -> Result<()> {
    let svc = service_name();
    run_systemctl_user(["start", &svc])?;
    println!("Started {svc}.");
    Ok(())
}

fn daemon_stop(paths: ClientPaths, yes: bool) -> Result<()> {
    let svc = service_name();
    if !is_user_service_active()? {
        println!("{svc} is already stopped.");
        return Ok(());
    }

    println!("Warning: stopping the daemon will alert people monitoring you.");

    if !yes && !prompt_yes_no("Continue stopping the daemon?", false)? {
        println!("Daemon stop cancelled.");
        return Ok(());
    }

    let sock = paths.state_dir.join("daemon.sock");
    let mut client =
        ClientController::connect(&sock).context("failed to connect to daemon (is it running?)")?;
    client
        .request_user_stop("cli_daemon_stop")
        .context("failed to record stop intent")?;

    // `request_user_stop` only publishes onto the IPC socket — it doesn't wait
    // for the daemon to actually read and dispatch it. Without this round trip,
    // `systemctl stop`'s SIGTERM can arrive before the daemon's next loop
    // iteration drains the socket, silently dropping the immediate/emailed
    // UserStop alert. `get_status` is a synchronous request/response over the
    // same ordered connection, so its reply guarantees the daemon has already
    // processed (and persisted) the earlier UserStopRequested.
    let _ = client.get_status();

    run_systemctl_user(["stop", &svc])?;

    println!("Stopped {svc}.");
    Ok(())
}

fn is_user_service_active() -> Result<bool> {
    let svc = service_name();
    let status = Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", &svc])
        .status()
        .with_context(|| format!("failed to query {svc} status with systemctl --user"))?;

    match status.code() {
        Some(0) => Ok(true),
        Some(3) => Ok(false),
        _ => Err(anyhow::anyhow!(
            "systemctl --user is-active --quiet {svc} exited with status {}",
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
        #[cfg(debug_assertions)]
        DevCommands::Send(args) => dev_send(paths, args),
    }
}

/// Builds the `UploadKind` for a single `dev send` log type. Returns `None` for
/// `screenshot` (handled separately) and errors on unknown types/kinds.
#[cfg(debug_assertions)]
fn build_send_kind(args: &SendLogArgs) -> Result<UploadKind> {
    let parse_enum = |value: &str| -> Result<serde_json::Value> {
        Ok(serde_json::Value::String(value.to_string()))
    };
    match args.log_type.as_str() {
        "lifecycle_alert" => {
            let reason = args
                .reason
                .as_deref()
                .context("--reason is required for --type lifecycle_alert")?;
            let reason: AlertReason = serde_json::from_value(parse_enum(reason)?)
                .with_context(|| format!("unknown alert reason: {reason}"))?;
            Ok(UploadKind::LifecycleAlert { reason })
        }
        "alert" => Ok(UploadKind::Alert {
            message: args
                .message
                .clone()
                .unwrap_or_else(|| "Developer test alert".to_string()),
        }),
        "screenshot_skipped" => {
            let reason = args.reason.as_deref().unwrap_or("static_screen");
            let reason: ScreenshotSkipReason = serde_json::from_value(parse_enum(reason)?)
                .with_context(|| format!("unknown screenshot skip reason: {reason}"))?;
            Ok(UploadKind::ScreenshotSkipped { reason })
        }
        "capture_failed" => Ok(UploadKind::CaptureFailed),
        "dev" => Ok(UploadKind::Dev {
            title: args
                .title
                .clone()
                .unwrap_or_else(|| "Developer CLI log".to_string()),
            details: args.details.clone(),
        }),
        other => anyhow::bail!(
            "unsupported --type {other:?} (expected: lifecycle_alert, screenshot_skipped, alert, capture_failed, dev, screenshot, or all)"
        ),
    }
}

/// Every concrete log variant `dev send --all` queues — one per web log icon.
#[cfg(debug_assertions)]
fn all_send_kinds() -> Vec<UploadKind> {
    use AlertReason::*;
    let alerts = [LateWakeup, UserStop]
        .into_iter()
        .map(|reason| UploadKind::LifecycleAlert { reason });
    let skips = [
        ScreenshotSkipReason::StaticScreen,
        ScreenshotSkipReason::LockedOrScreensaver,
    ]
    .into_iter()
    .map(|reason| UploadKind::ScreenshotSkipped { reason });
    alerts
        .chain(skips)
        .chain([
            UploadKind::Alert {
                message: "Developer test alert".to_string(),
            },
            UploadKind::CaptureFailed,
            UploadKind::Dev {
                title: "Developer CLI log".to_string(),
                details: None,
            },
        ])
        .collect()
}

#[cfg(debug_assertions)]
fn dev_send(paths: ClientPaths, args: SendLogArgs) -> Result<()> {
    let client = connect_to_daemon(&paths)?;

    let kinds = if args.log_type == "all" {
        // Screenshots are excluded here: the running daemon already produces real
        // screenshots, and capture/spooling depends on the platform environment.
        // Use `dev send --type screenshot` explicitly to queue one.
        all_send_kinds()
    } else if args.log_type == "screenshot" {
        let shot = LinuxPlatformHooks::new()
            .take_screenshot()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        vec![UploadKind::Screenshot {
            image: shot.bytes,
            content_type: shot.content_type,
            skin_detection: None,
            nsfw_detection: None,
        }]
    } else {
        vec![build_send_kind(&args)?]
    };

    let count = kinds.len();
    for kind in kinds {
        client
            .queue_upload(Upload {
                risk: args.risk,
                kind,
            })
            .context("failed to queue developer log")?;
    }

    println!(
        "Queued {count} log(s) in the next batch with risk {}. Run `virtue dev upload-batch` to send them now.",
        format_risk(args.risk)
    );
    Ok(())
}

/// Connect to the running daemon over IPC. Dev commands queue events into the
/// daemon's own live batch/hash pipeline rather than editing `event_state.json`
/// directly — the daemon holds that state in memory and rewrites the file on
/// every ping (~1s), so a direct edit would race with (or be silently
/// clobbered by) the daemon's next write.
fn connect_to_daemon(paths: &ClientPaths) -> Result<ClientController<virtue_core::RemoteEventBus>> {
    let sock = paths.state_dir.join("daemon.sock");
    ClientController::connect(&sock)
        .context("failed to connect to daemon (is it running? try `virtue daemon start`)")
}

fn dev_upload_log(paths: ClientPaths, args: DeveloperEventArgs) -> Result<()> {
    let title = args
        .title
        .unwrap_or_else(|| "Developer CLI log".to_string());
    let client = connect_to_daemon(&paths)?;
    // risk >= 1.0 routes through the encrypted batch plus an immediate POST /d/notify.
    client
        .queue_upload(Upload {
            risk: 1.0_f32,
            kind: UploadKind::Dev {
                title,
                details: args.details,
            },
        })
        .context("failed to queue developer log")?;

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
    let client = connect_to_daemon(&paths)?;
    client
        .queue_upload(Upload {
            risk: args.risk,
            kind: UploadKind::Dev {
                title,
                details: args.details,
            },
        })
        .context("failed to queue developer log")?;

    println!(
        "Queued developer log in the next batch with risk {}.",
        format_risk(args.risk)
    );
    Ok(())
}

fn dev_add_screenshot(paths: ClientPaths, args: DeveloperEventArgs) -> Result<()> {
    let screenshot = LinuxPlatformHooks::new()
        .take_screenshot()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let client = connect_to_daemon(&paths)?;
    client
        .queue_upload(Upload {
            risk: args.risk,
            kind: UploadKind::Screenshot {
                image: screenshot.bytes,
                content_type: screenshot.content_type,
                skin_detection: None,
                nsfw_detection: None,
            },
        })
        .context("failed to queue developer screenshot")?;

    println!(
        "Captured and queued a developer screenshot with risk {}.",
        format_risk(args.risk)
    );
    Ok(())
}

fn dev_upload_batch(paths: ClientPaths) -> Result<()> {
    let mut client = connect_to_daemon(&paths)?;

    let initial_pending = client
        .get_status()
        .context("failed to query daemon status")?
        .pending_request_count;
    if initial_pending == 0 {
        println!("No pending batch items to upload.");
        return Ok(());
    }

    client
        .flush_batch_now()
        .context("failed to request batch flush")?;

    // The flush is processed asynchronously on the daemon's next ping cycle
    // (≤1s) plus however long the network upload takes. Give it a full cycle
    // before the first check, then keep polling as long as it's still making
    // progress, up to a generous cap.
    std::thread::sleep(Duration::from_millis(1200));
    let mut remaining = client
        .get_status()
        .context("failed to query daemon status")?
        .pending_request_count;
    let deadline = Instant::now() + Duration::from_secs(10);
    while remaining > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        let seen = client
            .get_status()
            .context("failed to query daemon status")?
            .pending_request_count;
        if seen == remaining {
            break;
        }
        remaining = seen;
    }

    let attempted = initial_pending.saturating_sub(remaining);
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
    if let Ok(mut client) = ClientController::connect(&sock)
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
        assert!(matches!(
            cli.command,
            Commands::Login {
                email: None,
                device_name: None
            }
        ));
    }

    #[test]
    fn cli_accepts_login_device_name_flag() {
        let cli = Cli::try_parse_from(["virtue", "login", "--device-name", "My Box"])
            .expect("login --device-name should parse");
        assert!(matches!(
            cli.command,
            Commands::Login {
                email: None,
                device_name: Some(name)
            } if name == "My Box"
        ));
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
