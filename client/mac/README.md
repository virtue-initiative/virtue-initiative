# Virtue macOS Client

This client has two modes in one binary:

- Tray app (default): menu bar icon + login/logout dialogs
- Daemon (`daemon`): background capture + upload worker

## Behavior

- On tray app launch, it installs/starts a LaunchAgent (`org.virtueinitiative.virtue.daemon`) so the daemon restarts on login/reboot.
- Login/logout flows use the shared device API (`/d/...`) and shared core auth command behavior.
- Daemon idles until login is complete (device token + device ID + E2EE key).
- Menu action `Open Virtue`:
  - If logged out: prompts for email/password.
  - If logged in: shows signed-in state with `Restart daemon` / `Logout`.
- Tray menu action `Close (Will Send Alert)` sends a `manual_override` alert (best effort), stops the daemon launch agent, and exits the tray app.
- Closing behavior:
  - `Close (Will Send Alert)` is a full stop: tray icon exits and background daemon is stopped.
  - After closing, opening the installed `/Applications/Virtue.app` starts both the tray icon and daemon again.

## Lifecycle logs

macOS daemon alert logs include:

- `daemon_start` when the daemon starts.
- `daemon_stop_signal` when SIGTERM/SIGINT is received.
- `system_startup` when kernel `kern.boottime` changes since last daemon run.
- `system_shutdown` when AppKit posts `NSWorkspaceWillPowerOffNotification` and daemon receives SIGTERM.

`system_shutdown` is best-effort and may still be missed on abrupt power loss or forced termination.

## Local API Override

macOS reads runtime overrides from:

`~/Library/Application Support/virtue/config.json`

Example:

```json
{
  "api_base_url": "http://localhost:8787",
  "capture_interval_seconds": 120,
  "batch_window_seconds": 900
}
```

Apply after changing the file:

```bash
launchctl kickstart -k gui/$(id -u)/org.virtueinitiative.virtue.daemon
```

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

`client/target/macos/Virtue-<version>.dmg`
