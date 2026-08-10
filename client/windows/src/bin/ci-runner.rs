// CI-only helper: drives an end-to-end login/capture/batch cycle in-process.
//
// Unlike Linux (`virtue login` CLI) and macOS (a daemon binary + a
// `virtue-mac-ci-login` helper talking to it over an IPC socket), the
// Windows client has no standalone daemon process or CLI at all --
// `virtue_windows` is purely a cdylib the WinUI app loads via P/Invoke (see
// `RustInteropClient.cs`), and monitoring/login both happen as in-process
// calls against a background thread that same process spawns
// (`resident_monitor::start_monitoring`, `SessionManager::login_blocking`).
//
// This binary reproduces exactly what the WinUI app's `SessionViewModel`
// does at startup and login time -- write runtime config overrides, start
// monitoring, then log in (see `SessionViewModel.InitializeAsync`/
// `LoginAsync`) -- then blocks for a fixed run window so the monitor's
// background thread can actually capture/hash/batch/upload before the
// process exits (which would otherwise kill that thread immediately).
//
// Usage:
//   virtue-windows-ci-runner --api-base-url <url> --email <email> --password <password>
//     --device-name <name> [--capture-interval-seconds N] [--batch-window-seconds N]
//     --run-duration-seconds N
//
// Respects the PROGRAMDATA environment variable for isolation, same as the
// product code (`ClientPaths::discover`).

use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use clap::Parser;

use virtue_windows::config::{ClientPaths, RuntimeConfigOverrides, save_runtime_overrides};
use virtue_windows::resident_monitor;
use virtue_windows::session::SessionManager;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    api_base_url: String,
    #[arg(long)]
    email: String,
    #[arg(long)]
    password: String,
    #[arg(long = "device-name")]
    device_name: String,
    #[arg(long, default_value_t = 15)]
    capture_interval_seconds: u64,
    #[arg(long, default_value_t = 15)]
    batch_window_seconds: u64,
    #[arg(long)]
    run_duration_seconds: u64,
}

fn main() -> ExitCode {
    let args = Args::parse();

    let paths = match ClientPaths::discover().and_then(|paths| {
        paths.ensure_dirs()?;
        Ok(paths)
    }) {
        Ok(paths) => paths,
        Err(err) => {
            eprintln!("ci-runner: failed to resolve client paths: {err:#}");
            return ExitCode::FAILURE;
        }
    };

    let overrides = RuntimeConfigOverrides {
        api_base_url: Some(args.api_base_url),
        capture_interval_seconds: Some(args.capture_interval_seconds),
        batch_window_seconds: Some(args.batch_window_seconds),
    };
    if let Err(err) = save_runtime_overrides(&paths.runtime_config_file, &overrides) {
        eprintln!("ci-runner: failed to write runtime config: {err:#}");
        return ExitCode::FAILURE;
    }

    if let Err(err) = resident_monitor::start_monitoring() {
        eprintln!("ci-runner: failed to start monitoring: {err:#}");
        return ExitCode::FAILURE;
    }

    let manager = SessionManager { paths };
    match manager.login_blocking(&args.email, &args.password, &args.device_name) {
        Ok(device_id) => println!("ci-runner: logged in, device_id={device_id}"),
        Err(err) => {
            eprintln!("ci-runner: login failed: {err:#}");
            return ExitCode::FAILURE;
        }
    }

    println!(
        "ci-runner: running for {}s to allow capture/batch/hash activity",
        args.run_duration_seconds
    );
    thread::sleep(Duration::from_secs(args.run_duration_seconds));

    if let Err(err) = resident_monitor::stop_monitoring() {
        eprintln!("ci-runner: failed to stop monitoring cleanly: {err:#}");
    }

    ExitCode::SUCCESS
}
