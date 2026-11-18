# Virtue macOS client

A SwiftUI menu bar app plus a background daemon that runs the shared Rust core. It ships as a
notarized `.dmg` and updates itself with Sparkle. What it does for users is specified in
`client/SPEC.md` (CLIENT-024 to CLIENT-026).

- [DESIGN.md](DESIGN.md): the two-process model, the Screen Recording permission and silent
  updates.
- [RELEASING.md](RELEASING.md): update channels, signing and certificate renewal.

## Layout

- `app/Sources/`: the SwiftUI app. `UpdateController.swift` drives Sparkle.
- `rust/`: the Rust bridge the app calls (`virtue-mac-ffi`).
- `src/`: the `virtue-mac` crate. Its binary ships in the bundle as `virtue-daemon`. `capture.rs` calls
  `screencapture`, `launch_agent.rs` manages the LaunchAgent, `power.rs` watches for wake.
- `project.yml`: the XcodeGen project. `scripts/generate-project.sh` regenerates
  `VirtueMac.xcodeproj` from it.
- `scripts/build-app.sh`, `scripts/build-dmg.sh`: build the app bundle and the disk image.

## Build from Linux

`just mac-remote` compiles the Rust on Linux and has a Mac compile the Swift, link and sign over
SSH. The app lands in `/Users/Shared/virtue-builds/<worktree>/` on the Mac, signed ad hoc. The
setup it needs is in "Building the macOS and iOS apps from Linux" in `docs/development.md`. Run
`client/scripts/remote-apple-build.sh --help` for all options.

## Build on a Mac

You need Xcode and Rust.

```bash
just mac-build --arch arm64 --profile debug   # client/target/macos/Virtue.app
./client/mac/scripts/build-dmg.sh             # client/target/macos/virtue-macos-<version>.dmg
```

`build-app.sh` builds a universal release app by default, which is slow. Pass `--arch` and
`--profile debug` while iterating. With no `CODESIGN_IDENTITY` set, it signs with the team's
Developer ID certificate if the keychain has one, and ad hoc otherwise.

Moving between an ad-hoc build and a Developer ID build makes macOS ask for Screen Recording
again. That's expected (see [DESIGN.md](DESIGN.md#keeping-the-permission-across-updates)).

`docs/ci-checks.md` lists the exact commands macOS CI runs, including the Swift tests in
`client/shared-swift/` and the `swift-format` check.

## Pointing it at a local API

Set `VIRTUE_DEFAULT_API_URL` in the repo-root `.env` and rebuild. See
[`client/README.md`](../README.md). A local build never turns on auto-update (see
[RELEASING.md](RELEASING.md#turning-auto-update-on)), so it can't replace itself with a release.

## Integration test

```bash
bun client/mac/scripts/integration-test.ts
```

Runs on macOS only. It starts a local API, runs the real daemon with a mock capture and checks
that screenshots reach the database. [DESIGN.md](DESIGN.md#integration-test) explains why the
capture is mocked.

## Files on a running machine

| Path                                                              | What                                            |
| ----------------------------------------------------------------- | ----------------------------------------------- |
| `~/Library/Application Support/virtue/state/event_state.json`     | The core's state (CORE-016)                     |
| `~/Library/Application Support/virtue/ui_state.json`              | The remembered sign-in email                    |
| `~/Library/Logs/virtue.<date>.log`                                | The daemon's daily log (Reveal Logs in the app) |
| `~/Library/LaunchAgents/org.virtueinitiative.virtue.daemon.plist` | The LaunchAgent that runs the daemon            |

`Contents/MacOS/virtue-daemon status` prints the daemon's state from the command line.

## Troubleshooting

- **The window keeps asking for Screen Recording.** Check that Virtue is on in System Settings >
  Privacy & Security > Screen & System Audio Recording. A build signed differently from the one
  that was granted counts as a new app.
- **"Virtue background service stopped unexpectedly".** The daemon isn't running. Relaunching
  the app restarts the LaunchAgent. `launchctl print gui/$(id -u)/org.virtueinitiative.virtue.daemon`
  shows its state.
- **An update didn't install.** Read the logs, and check that the build was made with
  auto-update on.
