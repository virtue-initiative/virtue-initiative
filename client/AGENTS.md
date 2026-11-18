# AGENTS.md — client/

Multi-platform Rust monitoring client. All platforms share `core/`; each platform crate is a thin wrapper that supplies raw screen data and OS hooks, then runs `core`'s `Daemon::run_forever()` loop. Start with [README.md](README.md) for the platform table and where each platform's docs live, and `core/architecture.md` for how the core works.

## Rules

- **Policy belongs in `core`.** Capture cadence, risk scoring, tamper detection, uploads and status all come from the core (CLIENT-001). A platform must not reimplement them. If a platform needs new behavior, add a hook or a `Daemon` method, not a platform-side copy.
- **A status field goes in `core/src/module/status.rs`**, which every platform's status page renders (CORE-010), not in one platform's UI code.
- **Always name the package**: `cargo build -p virtue-core`, `cargo test -p virtue-linux`. Several workspace members only compile on their own OS, so a bare `cargo build` in `client/` fails.
- **A `client/core` change affects every platform.** Run the Linux checks at minimum, and the others you can (see `docs/ci-checks.md`).
- **Platform-specific design decisions are in each platform's `DESIGN.md`.** Read it before changing that platform's process model, updater or permissions handling.

## Where each platform enters the core

| Platform | Entry point                       | How the UI reaches the daemon                    |
| -------- | --------------------------------- | ------------------------------------------------ |
| Linux    | `linux/src/daemon.rs`             | Separate process, IPC socket (`core/src/ipc.rs`) |
| macOS    | `mac/src/daemon.rs`               | Separate process, IPC socket                     |
| Windows  | `windows/src/resident_monitor.rs` | In-process, through `windows/src/ffi.rs`         |
| Android  | `android/rust/src/lib.rs`         | In-process, through JNI                          |
| iOS      | `ios/rust/src/lib.rs`             | In-process, through C FFI; lifecycle check off   |

## Key invariants (don't change without cross-component review)

See `../AGENTS.md` (repo root) for wire format constraints shared with the TypeScript web app.
The most dangerous files are `core/src/crypto.rs` and `core/src/module/upload/batch.rs`.
