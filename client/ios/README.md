# Virtue iOS client

A SwiftUI app plus a Safari Web Extension that captures the visible Safari tab. The shared Rust
core runs inside the extension. Only Safari is monitored. It ships through TestFlight. What it
does for users is specified in `client/SPEC.md` (CLIENT-032).

- [DESIGN.md](DESIGN.md): why the core runs in the extension and what that limits.
- [RELEASING.md](RELEASING.md): TestFlight distribution and export compliance.

## Layout

- `app/Sources/`: the SwiftUI app. `MonitoringCoordinator.swift` tracks the extension's state,
  `SafariSetupGuide.swift` walks the user through turning it on, `NativeBridge.swift` wraps
  the Rust calls.
- `app/SafariWebExtension/`: the extension. `SafariWebExtensionHandler.swift` runs the daemon
  and receives frames. In `app/SafariWebExtension/Resources/`, `background.js` captures the tab and `content.js` triggers
  a capture from the page.
- `app/Shared/VirtueShared.swift`: keys and constants both targets use.
- `rust/`: the C-FFI bridge crate (`rust/src/lib.rs`).
- `project.yml`: the XcodeGen project. `scripts/generate-project.sh` regenerates
  `VirtueIOS.xcodeproj` from it.

## Build from Linux

`just ios-remote` compiles the Rust on Linux and has a Mac build the app over SSH:

```bash
just ios-remote --run                                   # build and launch in the Mac's booted simulator
just ios-remote --destination device --profile release  # device build
```

The setup it needs is in "Building the macOS and iOS apps from Linux" in `docs/development.md`.

## Build on a Mac

You need Xcode and Rust.

```bash
./client/ios/scripts/generate-project.sh                                       # after editing project.yml
just ios-build --destination "generic/platform=iOS Simulator"                   # simulator build
./client/ios/scripts/run-on-device.sh --team-id <APPLE_TEAM_ID>                 # build and run on a connected iPhone
```

Run each script with `--help` for its options. `docs/ci-checks.md` lists the exact commands iOS CI runs.

## Trying it

1. Launch the app and sign in.
2. In Settings, turn on the Virtue Safari extension and allow it on All Websites.
3. Browse in Safari. The app's Check Extension button confirms the extension is reporting in.

## Pointing it at a local API

Set `VIRTUE_DEFAULT_API_URL` in the repo-root `.env` and rebuild. See
[`client/README.md`](../README.md). The simulator shares the Mac's network, so a device or
simulator on another machine needs the API started on all interfaces (see `api/README.md`).
