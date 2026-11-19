# Virtue client apps

The monitoring apps for Linux, macOS, Windows, Android and iOS. Every platform runs the same Rust
core (`core/`), which owns screenshot scheduling, risk scoring, tamper detection, encryption,
batching and upload. Each platform app supplies only what the core can't: screen capture, OS
signals, a way to sign in, a status display and OS integration (CLIENT-001).

## Platforms

| Platform | App                                                     | How it captures                           | Updates                         | Docs                                   |
| -------- | ------------------------------------------------------- | ----------------------------------------- | ------------------------------- | -------------------------------------- |
| Linux    | `virtue` CLI, a systemd user service and a tray icon    | `import`/`maim` on X11, `grim` on Wayland | `.deb` from GitHub Releases     | [linux/README.md](linux/README.md)     |
| macOS    | SwiftUI menu bar app plus a `virtue-daemon` LaunchAgent | `screencapture`                           | Sparkle                         | [mac/README.md](mac/README.md)         |
| Windows  | WinUI 3 tray app hosting the core in-process            | GDI                                       | Microsoft Store                 | [windows/README.md](windows/README.md) |
| Android  | Kotlin app; the Accessibility service runs the core     | `AccessibilityService.takeScreenshot()`   | Self-update from a landing feed | [android/README.md](android/README.md) |
| iOS      | SwiftUI app plus a Safari Web Extension                 | The visible Safari tab only               | TestFlight                      | [ios/README.md](ios/README.md)         |

Each platform folder has up to three docs:

- `README.md`: build, run and test it on a dev machine.
- `DESIGN.md`: why the platform-specific parts work the way they do. Read it before changing
  them.
- `RELEASING.md`: signing, store submission and the release pipeline.

## What to read

- [`SPEC.md`](SPEC.md): what users see and can do in every app (`CLIENT-` IDs). The source of
  truth for behavior.
- [`core/SPEC.md`](core/SPEC.md): what the core does (`CORE-` IDs).
- [`core/architecture.md`](core/architecture.md): how the core is built.
- [`../docs/overview.md`](../docs/overview.md): how the clients fit with the API, hash server
  and web app.

## Pointing a build at a local API

The API URL, capture interval and batch window are baked in at compile time. No platform reads
them from a config file at runtime (CLIENT-001). To build against a local API:

1. Copy `.env.example` at the repo root to `.env`.
2. Set `VIRTUE_DEFAULT_API_URL` to the API URL `./scripts/launch.sh` prints. Optionally set
   `VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS` (minimum 15) and
   `VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS` (minimum 1).
3. Rebuild.

Real environment variables win over `.env`, and `.env` wins over `~/.config/virtue-dev.env` (see
"Centralized dev config" in [`docs/development.md`](../docs/development.md)). `client/core/build.rs` reads them,
so this works the same on every platform.

From an Android emulator, use `http://10.0.2.2:<port>` to reach the host. `localhost` inside the
emulator is the emulator itself.

## Building from Linux

Most development happens on Linux:

- Linux builds natively.
- macOS and iOS: `just mac-remote` and `just ios-remote` compile the Rust here and the Swift on
  a Mac over SSH (see "Building the macOS and iOS apps from Linux" in [`docs/development.md`](../docs/development.md)).
- Windows builds in a VM over SSH. See [windows/README.md](windows/README.md).
- Android builds natively with the Android SDK.

`just --list` shows the build, install and test recipes for every platform.

## Workspace layout

- `core/`: the shared Rust core.
- `linux/`, `mac/`, `windows/`: platform crates.
- `mac/rust/`, `android/rust/`, `ios/rust/`: the bridges the Swift and Kotlin apps call into.
- `text-detection/`: OCR over captured screenshots, used for redaction (CLIENT-019).
- `shared-swift/`: Swift code shared by the macOS and iOS apps.

All the Rust crates are members of one Cargo workspace (`Cargo.toml`). Several only compile for
their own platform, so always name the package: `cargo build -p virtue-core`, never a bare
`cargo build`.

- `scripts/`: versioning, release publishing and cross-platform build helpers.
- `version.properties`: the version shared by the clients, API and hash server. After editing
  it, run `scripts/update-version.sh` to copy it into every file that hardcodes it.
