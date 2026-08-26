mod capture;
mod config;
mod daemon;
mod tray;

use std::fs;
use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::process::Command;
use std::process::ExitCode;
use std::process::Stdio;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
#[cfg(debug_assertions)]
use virtue_core::ScreenshotSkipReason;
use virtue_core::ipc::ClientController;
use virtue_core::{ScreenshotHooks, Upload, UploadKind};

use crate::capture::{CaptureBackend, LinuxPlatformHooks, detect_backend, probe_backend};
use crate::config::{ClientPaths, build_core_config, default_device_name, load_service_status};

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
    #[command(
        about = "Force an immediate screenshot capture and upload, same as Force Screenshot & Upload on other platforms"
    )]
    ForceScreenshot,
    #[command(about = "Report an issue to the Virtue Initiative team")]
    ReportIssue {
        /// Skips the interactive prompt; the report is submitted as-is.
        #[arg(long)]
        message: Option<String>,
        #[arg(long)]
        contact_email: Option<String>,
        /// Skips the "here's what will be sent" confirmation prompt.
        #[arg(long)]
        yes: bool,
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
    /// Skip reason (snake_case) when --type screenshot_skipped, e.g. static_screen.
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
        Commands::Status { json } => tokio::task::block_in_place(|| status(paths, json)),
        Commands::Dev { command } => tokio::task::block_in_place(|| dev(paths, command)),
        Commands::ForceScreenshot => tokio::task::block_in_place(|| force_screenshot(paths)),
        Commands::ReportIssue {
            message,
            contact_email,
            yes,
        } => tokio::task::block_in_place(|| report_issue(paths, message, contact_email, yes)),
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

fn report_issue(
    paths: ClientPaths,
    message: Option<String>,
    contact_email: Option<String>,
    yes: bool,
) -> Result<()> {
    let message = match message {
        Some(message) => message,
        None => {
            let mut rl = rustyline::DefaultEditor::new()?;
            rl.readline("Describe the issue: ")?
        }
    };
    let message = message.trim().to_string();
    if message.is_empty() {
        println!("No message entered; nothing was sent.");
        return Ok(());
    }

    let device_refresh_token = read_device_refresh_token(&paths);

    let contact_email = match contact_email {
        Some(email) => Some(email),
        None if device_refresh_token.is_some() => None,
        None => {
            let mut rl = rustyline::DefaultEditor::new()?;
            let entered = rl.readline("Contact email (optional, press Enter to skip): ")?;
            let entered = entered.trim();
            (!entered.is_empty()).then(|| entered.to_string())
        }
    };

    let platform_details = linux_platform_details();
    let logs = recent_logs();

    println!("This report will be sent to the Virtue Initiative team and will include:");
    println!("  - Your message: \"{message}\"");
    if let Some(email) = &contact_email {
        println!("  - Your contact email: {email}");
    }
    println!("  - Platform details: {platform_details}");
    if device_refresh_token.is_some() {
        println!("  - This device's identity and your account email (you're logged in)");
    }
    match &logs {
        Some(logs) => println!(
            "  - The last day of this device's operational logs ({} KB) from \
             `journalctl --user -u {}`: diagnostic entries only (errors, capture/upload status) \
             — no screenshots or screenshot content, and no window titles.",
            logs.len().div_ceil(1024),
            service_name(),
        ),
        None => println!("  - (no logs found to attach)"),
    }

    if !yes && !prompt_yes_no("Send this report?", true)? {
        println!("Report cancelled.");
        return Ok(());
    }

    let config = build_core_config(&paths);
    let api = virtue_core::api::HttpApiClient::new(&config)?;
    api.report_issue(
        device_refresh_token.as_deref(),
        &virtue_core::api::BugReportRequest {
            message: &message,
            contact_email: contact_email.as_deref(),
            platform: "linux",
            app_version: BUILD_LABEL,
            platform_details: Some(&platform_details),
        },
        logs.as_deref(),
    )
    .context("failed to submit bug report")?;

    println!("Thanks — your report was sent to the Virtue Initiative team.");
    Ok(())
}

/// Best-effort last day of this device's `virtue` user-service logs, redacted
/// (see `virtue_core::api::redact_secrets`) and trimmed to the API's
/// attachment size cap (keeping the most recent bytes, since those are the
/// most relevant to a just-encountered issue).
fn recent_logs() -> Option<Vec<u8>> {
    let output = Command::new("journalctl")
        .args([
            "--user",
            "-u",
            &service_name(),
            "--since",
            "-1 day",
            "--no-pager",
            "-o",
            "short-iso",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }

    let redacted = virtue_core::api::redact_secrets(&String::from_utf8_lossy(&output.stdout));
    let mut logs = redacted.into_bytes();
    if logs.len() > virtue_core::api::MAX_LOG_ATTACHMENT_BYTES {
        let start = logs.len() - virtue_core::api::MAX_LOG_ATTACHMENT_BYTES;
        logs.drain(0..start);
    }
    Some(logs)
}

/// Reads this device's refresh token straight off disk rather than through
/// the daemon, so `report-issue` still works (anonymously otherwise) even
/// when the daemon isn't running. See CORE-010's disk-fallback precedent in
/// `load_service_status`.
fn read_device_refresh_token(paths: &ClientPaths) -> Option<String> {
    let state_path = paths.state_dir.join("event_state.json");
    let state: virtue_core::DaemonState = virtue_core::load_state(&state_path).ok()?;
    state
        .auth
        .device_credentials
        .map(|creds| creds.refresh_token)
}

/// Best-effort OS description for `platform_details`: kernel release plus
/// `/etc/os-release`'s NAME/VERSION, e.g. "Linux 6.8.0-60-lowlatency;
/// Ubuntu 24.04.1 LTS".
fn linux_platform_details() -> String {
    let kernel = Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let os_release = fs::read_to_string("/etc/os-release")
        .ok()
        .map(|contents| parse_os_release(&contents));

    let mut parts = Vec::new();
    if let Some(kernel) = kernel {
        parts.push(format!("Linux {kernel}"));
    }
    if let Some((name, version)) = os_release {
        match (name, version) {
            (Some(name), Some(version)) => parts.push(format!("{name} {version}")),
            (Some(name), None) => parts.push(name),
            (None, _) => {}
        }
    }

    if parts.is_empty() {
        "unknown".to_string()
    } else {
        parts.join("; ")
    }
}

/// Parses `NAME=`/`VERSION=` out of `/etc/os-release` (each value optionally
/// double-quoted, per the os-release(5) format).
fn parse_os_release(contents: &str) -> (Option<String>, Option<String>) {
    let mut name = None;
    let mut version = None;
    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("NAME=") {
            name = Some(value.trim_matches('"').to_string());
        } else if let Some(value) = line.strip_prefix("VERSION=") {
            version = Some(value.trim_matches('"').to_string());
        }
    }
    (name, version)
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

    // `request_user_stop` already blocks until the daemon acks the stop
    // request; this extra round trip is just cheap insurance against
    // `systemctl stop`'s SIGTERM racing the daemon's persist of it.
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
        "user_stop" => Ok(UploadKind::UserStop),
        "screenshot_missed" => Ok(UploadKind::ScreenshotMissed),
        "system_login" => Ok(UploadKind::SystemLogin { utc_ms: now_ms() }),
        "system_logout" => Ok(UploadKind::SystemLogout { utc_ms: now_ms() }),
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
            "unsupported --type {other:?} (expected: user_stop, screenshot_missed, system_login, system_logout, screenshot_skipped, alert, capture_failed, dev, screenshot, or all)"
        ),
    }
}

/// Current UTC time in milliseconds — used as the `utc_ms` for dev-triggered
/// `system_login`/`system_logout` events, which have no real login/logout
/// to report.
#[cfg(debug_assertions)]
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Every concrete log variant `dev send --all` queues — one per web log icon.
#[cfg(debug_assertions)]
fn all_send_kinds() -> Vec<UploadKind> {
    let skips = [
        ScreenshotSkipReason::StaticScreen,
        ScreenshotSkipReason::LockedOrScreensaver,
    ]
    .into_iter()
    .map(|reason| UploadKind::ScreenshotSkipped { reason });
    let utc_ms = now_ms();
    skips
        .chain([
            UploadKind::UserStop,
            UploadKind::Alert {
                message: "Developer test alert".to_string(),
            },
            UploadKind::CaptureFailed,
            UploadKind::Dev {
                title: "Developer CLI log".to_string(),
                details: None,
            },
            UploadKind::ScreenshotMissed,
            UploadKind::SystemLogin { utc_ms },
            UploadKind::SystemLogout { utc_ms },
        ])
        .collect()
}

#[cfg(debug_assertions)]
fn dev_send(paths: ClientPaths, args: SendLogArgs) -> Result<()> {
    let mut client = connect_to_daemon(&paths)?;

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
        "Queued {count} log(s) in the next batch with risk {}. Run `virtue force-screenshot` to send them now.",
        format_risk(args.risk)
    );
    Ok(())
}

/// Connect to the running daemon over IPC. Dev commands queue events into the
/// daemon's own live batch/hash pipeline rather than editing `event_state.json`
/// directly — the daemon holds that state in memory and rewrites the file on
/// every ping (~1s), so a direct edit would race with (or be silently
/// clobbered by) the daemon's next write.
fn connect_to_daemon(paths: &ClientPaths) -> Result<ClientController> {
    let sock = paths.state_dir.join("daemon.sock");
    ClientController::connect(&sock)
        .context("failed to connect to daemon (is it running? try `virtue daemon start`)")
}

fn dev_upload_log(paths: ClientPaths, args: DeveloperEventArgs) -> Result<()> {
    let title = args
        .title
        .unwrap_or_else(|| "Developer CLI log".to_string());
    let mut client = connect_to_daemon(&paths)?;
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
    let mut client = connect_to_daemon(&paths)?;
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

fn force_screenshot(paths: ClientPaths) -> Result<()> {
    let mut client = connect_to_daemon(&paths)?;
    client
        .force_capture_now()
        .context("failed to request forced screenshot capture")?;
    println!(
        "Requested a forced screenshot capture (bypasses the interval gate, still honors \
         the lock/screensaver and dedup gates) and flushed the queue for immediate upload."
    );
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

#[cfg(test)]
mod tests {
    use super::{Cli, Commands, DaemonCommands, parse_os_release};
    use clap::Parser;

    #[test]
    fn cli_accepts_report_issue_command() {
        let cli = Cli::try_parse_from(["virtue", "report-issue"])
            .expect("report-issue command should parse");
        assert!(matches!(
            cli.command,
            Commands::ReportIssue {
                message: None,
                contact_email: None,
                yes: false,
            }
        ));
    }

    #[test]
    fn cli_accepts_report_issue_flags() {
        let cli = Cli::try_parse_from([
            "virtue",
            "report-issue",
            "--message",
            "Screenshots stopped uploading",
            "--contact-email",
            "me@example.com",
            "--yes",
        ])
        .expect("report-issue flags should parse");
        assert!(matches!(
            cli.command,
            Commands::ReportIssue { message: Some(m), contact_email: Some(e), yes: true }
                if m == "Screenshots stopped uploading" && e == "me@example.com"
        ));
    }

    #[test]
    fn parse_os_release_reads_name_and_version() {
        let contents = "NAME=\"Ubuntu\"\nVERSION=\"24.04.1 LTS (Noble Numbat)\"\nID=ubuntu\n";
        assert_eq!(
            parse_os_release(contents),
            (
                Some("Ubuntu".to_string()),
                Some("24.04.1 LTS (Noble Numbat)".to_string())
            )
        );
    }

    #[test]
    fn parse_os_release_handles_missing_fields() {
        assert_eq!(parse_os_release("ID=arch\n"), (None, None));
    }

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
