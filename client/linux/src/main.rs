mod capture;
mod config;
mod daemon;
mod tray;

use std::io::{self, Write};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::process::Command;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use virtue_core::storage::FileStateStore;
use virtue_core::{AuthState, EventData, LogEntry, MonitorService, ServiceRole, ServiceStatus};

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
    Status,
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
        Commands::Login { email } => login(paths, email),
        Commands::Logout { yes } => logout(paths, yes),
        Commands::Daemon { command } => daemon_command(paths, command).await,
        Commands::Status => status(paths),
        Commands::Dev { command } => dev(paths, command),
    }
}

async fn daemon_command(paths: ClientPaths, command: Option<DaemonCommands>) -> Result<()> {
    match command {
        None => daemon::run_daemon(&paths).await,
        Some(DaemonCommands::Start) => daemon_start(),
        Some(DaemonCommands::Stop { yes }) => daemon_stop(paths, yes),
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
        "Warning: logging out will alert people monitoring you and will recreate a new device on login."
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
    let mut config = build_core_config(&paths);
    config.refresh_from_runtime_file()?;
    let status = load_service_status(&store, &auth, &config)?;

    println!("logged_in: {}", auth.device_credentials.is_some());
    println!("running: {}", status.is_running);
    println!("pending_request_count: {}", status.pending_request_count);
    println!(
        "lifecycle_primary_service: {}",
        status.lifecycle.snapshot.primary_service.as_str()
    );
    println!(
        "lifecycle_computer_power: {}",
        status.lifecycle.snapshot.computer_power.as_str()
    );
    println!(
        "lifecycle_capture_permission: {}",
        status.lifecycle.snapshot.capture_permission.as_str()
    );
    println!(
        "lifecycle_capture_availability: {}",
        status.lifecycle.snapshot.capture_availability.as_str()
    );
    println!(
        "last_stop_origin: {}",
        status
            .lifecycle
            .last_stop_origin
            .map(|value| value.as_str())
            .unwrap_or("<none>")
    );
    println!(
        "last_lifecycle_risk: {}",
        status
            .lifecycle
            .last_emitted_risk
            .map(format_risk)
            .unwrap_or_else(|| "<none>".to_string())
    );
    if let Some(transition) = &status.lifecycle.last_transition {
        println!(
            "last_transition: {} {} -> {}",
            transition.domain.as_str(),
            transition.from,
            transition.to
        );
        println!("last_transition_origin: {}", transition.origin.as_str());
    } else {
        println!("last_transition: <none>");
    }
    println!(
        "device_id: {}",
        status.device_id.as_deref().unwrap_or("<none>")
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
    println!(
        "capability_startup: {}",
        status.lifecycle.capabilities.startup.as_str()
    );
    println!(
        "capability_shutdown: {}",
        status.lifecycle.capabilities.shutdown.as_str()
    );
    println!(
        "capability_suspend: {}",
        status.lifecycle.capabilities.suspend.as_str()
    );
    println!(
        "capability_wake: {}",
        status.lifecycle.capabilities.wake.as_str()
    );
    println!(
        "capability_explicit_user_stop: {}",
        status.lifecycle.capabilities.explicit_user_stop.as_str()
    );
    println!(
        "capability_capture_permission: {}",
        status.lifecycle.capabilities.capture_permission.as_str()
    );
    println!(
        "capability_capture_availability: {}",
        status.lifecycle.capabilities.capture_availability.as_str()
    );
    println!(
        "capability_user_login: {}",
        status.lifecycle.capabilities.user_login.as_str()
    );
    println!(
        "capability_user_logout: {}",
        status.lifecycle.capabilities.user_logout.as_str()
    );
    println!(
        "capability_capture_worker: {}",
        status.lifecycle.capabilities.capture_worker.as_str()
    );
    println!(
        "capability_next_boot_recovery: {}",
        status.lifecycle.capabilities.next_boot_recovery.as_str()
    );

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

    let mut service = MonitorService::setup(build_core_config(&paths), LinuxPlatformHooks::new())?;
    service.note_stop_requested_by_user(ServiceRole::PrimaryService, "cli_daemon_stop")?;

    if let Err(err) = run_systemctl_user(["stop", "virtue.service"]) {
        let _ = service.take_stop_intent(ServiceRole::PrimaryService);
        return Err(err);
    }

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
    service.send_log(LogEntry {
        ts: current_time_utc_ms()?,
        kind: "developer_log".to_string(),
        risk: Some(args.risk),
        data: developer_log_data("upload_log", &title, args.details.as_deref()),
    })?;

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
    service.queue_batch_log(
        "developer_log",
        Some(args.risk),
        developer_log_data("add_log", &title, args.details.as_deref()),
    )?;

    println!(
        "Queued developer log in the next batch with risk {}.",
        format_risk(args.risk)
    );
    Ok(())
}

fn dev_add_screenshot(paths: ClientPaths, args: DeveloperEventArgs) -> Result<()> {
    let title = args
        .title
        .unwrap_or_else(|| "Developer CLI screenshot".to_string());
    let mut service = MonitorService::setup(build_core_config(&paths), LinuxPlatformHooks::new())?;
    service.capture_batch_screenshot(
        "developer_screenshot",
        Some(args.risk),
        developer_log_data("add_screenshot", &title, args.details.as_deref()),
    )?;

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

fn developer_log_data(command: &str, title: &str, details: Option<&str>) -> EventData {
    let mut data = EventData::from_pairs([
        ("source".to_string(), "linux_dev_cli".to_string()),
        ("command".to_string(), command.to_string()),
        ("title".to_string(), title.to_string()),
    ]);
    if let Some(details) = details.filter(|value| !value.trim().is_empty()) {
        data.insert("details", serde_json::Value::String(details.to_string()));
    }
    data
}

fn current_time_utc_ms() -> Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before UNIX epoch")?;
    i64::try_from(duration.as_millis()).context("system clock overflow")
}

fn load_service_status(
    store: &FileStateStore,
    auth: &AuthState,
    config: &virtue_core::Config,
) -> Result<ServiceStatus> {
    let mut status = store.load_status()?.unwrap_or(ServiceStatus {
        is_authenticated: auth.device_credentials.is_some(),
        is_running: false,
        device_id: auth
            .device_credentials
            .as_ref()
            .map(|device| device.device_id.clone()),
        last_loop_at_ms: None,
        pending_request_count: 0,
        lifecycle: virtue_core::LifecycleStatus::for_platform(&config.platform_name),
    });
    status.is_running =
        status.is_running && has_fresh_status_heartbeat(&status, config, current_time_utc_ms()?);
    status.lifecycle.capabilities =
        virtue_core::LifecycleCapabilities::for_platform(&config.platform_name);
    Ok(status)
}

fn has_fresh_status_heartbeat(
    status: &ServiceStatus,
    config: &virtue_core::Config,
    now_ms: i64,
) -> bool {
    let Some(last_loop_at_ms) = status.last_loop_at_ms else {
        return false;
    };

    let heartbeat_window_ms = status_heartbeat_window(config).as_millis() as i64;
    now_ms.saturating_sub(last_loop_at_ms) <= heartbeat_window_ms
}

fn status_heartbeat_window(config: &virtue_core::Config) -> Duration {
    let base_interval = config
        .screenshot_interval
        .min(config.batch_interval)
        .max(Duration::from_secs(30));
    base_interval.checked_mul(2).unwrap_or(base_interval)
}

#[cfg(test)]
mod tests {
    use super::{
        Cli, Commands, DaemonCommands, has_fresh_status_heartbeat, status_heartbeat_window,
    };
    use clap::Parser;
    use std::path::PathBuf;
    use std::time::Duration;
    use virtue_core::{Config, ServiceStatus};

    fn test_config() -> Config {
        Config::new(
            "https://example.invalid",
            "test-device",
            "linux",
            PathBuf::from("/tmp/virtue-status-test"),
            None,
            Duration::from_secs(30),
            Duration::from_secs(60),
        )
    }

    fn test_status(last_loop_at_ms: Option<i64>) -> ServiceStatus {
        ServiceStatus {
            is_authenticated: true,
            is_running: true,
            device_id: Some("device-1".to_string()),
            last_loop_at_ms,
            pending_request_count: 0,
            lifecycle: virtue_core::LifecycleStatus::for_platform("linux"),
        }
    }

    #[test]
    fn status_heartbeat_window_uses_fastest_loop_interval_with_grace() {
        let config = test_config();

        assert_eq!(status_heartbeat_window(&config), Duration::from_secs(60));
    }

    #[test]
    fn heartbeat_is_stale_when_last_loop_is_too_old() {
        let config = test_config();
        let status = test_status(Some(1_000));

        assert!(!has_fresh_status_heartbeat(&status, &config, 62_000));
    }

    #[test]
    fn heartbeat_is_fresh_when_last_loop_is_recent() {
        let config = test_config();
        let status = test_status(Some(5_000));

        assert!(has_fresh_status_heartbeat(&status, &config, 64_000));
    }

    #[test]
    fn heartbeat_is_stale_when_status_has_never_looped() {
        let config = test_config();
        let status = test_status(None);

        assert!(!has_fresh_status_heartbeat(&status, &config, 64_000));
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
