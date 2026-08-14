# Virtue macOS Client

This client has two modes in one binary:

- Tray app (default): menu bar icon + login/logout dialogs
- Daemon (`daemon`): background capture + upload worker

## Behavior

- On tray app launch, it installs/starts a LaunchAgent (`org.virtueinitiative.virtue.daemon`) so the daemon starts immediately and is registered for future login/reboot launches.
- Login/logout flows use the shared device API (`/d/...`) and shared core auth command behavior.
- Daemon idles until login is complete (device token + device ID + E2EE key).
- Menu action `Open Virtue`:
  - If logged out: prompts for email/password.
  - If logged in: shows signed-in state with `Stop Monitoring` / `Logout`.
- `Stop Monitoring` asks for confirmation, records an explicit user stop intent, unregisters the daemon LaunchAgent, and exits the tray app.
- `Logout` asks for confirmation, alerts observers through the shared core logout flow, clears local login state, and unregisters the daemon LaunchAgent.
- Closing behavior:
  - `Stop Monitoring` is a full stop: tray icon exits, background daemon is stopped, and startup relaunch is disabled until the app is opened again.
  - `Logout` also disables startup relaunch until the app is opened again.
  - Opening the Virtue app again recreates/re-enables the LaunchAgent and starts background monitoring.

## Lifecycle logs

macOS lifecycle behavior follows the shared core lifecycle model, analogous to Linux:

- Service pings are recorded every 60 seconds so forced stops can be detected on the next start.
- Startup is detected from `kern.boottime`.
- Shutdown is best-effort via `NSWorkspaceWillPowerOffNotification` plus the daemon stop signal.
- If launchd delivers the stop signal before the power-off notification, the next boot upgrades a recent `unknown` stop marker into `system_shutdown` so reboot cycles stay zero-risk.
- Suspend and wake are tracked from `NSWorkspaceWillSleepNotification` / `NSWorkspaceDidWakeNotification`.
- Explicit tray-initiated stop prompts for confirmation before recording a user-requested stop.
- Logout prompts for confirmation before sending the shared core logout alert.

The tray app waits for the daemon to come up, but it now stays open and keeps polling if startup confirmation is delayed instead of immediately failing closed after a short timeout.

`system_shutdown` is best-effort and may still be missed on abrupt power loss or forced termination.

## Local configuration

There are no runtime config overrides — `api_base_url`, `capture_interval_seconds`, and
`batch_window_seconds` are compile-time constants baked into the binary via `env!()`
(see `client/core/build.rs`). To set local values for development, copy
`client/.env.example` to `client/.env` (gitignored) and edit it; real process/CI env
vars always take precedence over that file. Rebuild for changes to take effect.

The mac client stores shared core state under:

`~/Library/Application Support/virtue/state`

## Screen capture permission

macOS may block screenshot capture until Screen Recording permission is granted for the app/binary.
If captures fail, grant permission under:

`System Settings -> Privacy & Security -> Screen Recording`

## Build

From `client/`:

```bash
cargo build --release -p virtue-mac
```

## Build `.app`

```bash
./mac/scripts/build-app.sh
```

Creates:

`client/target/macos/Virtue.app`

## Build `.dmg` (drag to Applications)

```bash
./mac/scripts/build-dmg.sh
```

Creates:

`client/target/macos/virtue-macos-<version>.dmg`
