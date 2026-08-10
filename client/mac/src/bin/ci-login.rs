// CI-only helper: logs in over the daemon's IPC socket non-interactively.
//
// Unlike Linux's `virtue login`, macOS login normally happens through the SwiftUI
// app's FFI bridge (`virtue_mac_native_login` in `mac/rust/src/lib.rs`), which
// itself just does what this binary does: connect to `daemon.sock` and call
// `ClientController::login`. There's no interactive terminal password prompt to
// work around here, so — unlike Linux's `ci-login.ts` — no pty tricks are needed.
//
// Usage:
//   ci-login --socket <path-to-daemon.sock> --email <email> --password <password> --device-name <name>

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use virtue_core::ClientController;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    socket: PathBuf,
    #[arg(long)]
    email: String,
    #[arg(long)]
    password: String,
    #[arg(long = "device-name")]
    device_name: String,
}

fn main() -> ExitCode {
    let args = Args::parse();

    let mut client = match ClientController::connect(&args.socket) {
        Ok(client) => client,
        Err(err) => {
            eprintln!(
                "ci-login: failed to connect to daemon at {}: {err}",
                args.socket.display()
            );
            return ExitCode::FAILURE;
        }
    };

    match client.login(&args.email, &args.password, Some(&args.device_name)) {
        Ok(device_id) => {
            println!("ci-login: logged in, device_id={device_id}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("ci-login: login failed: {err}");
            ExitCode::FAILURE
        }
    }
}
