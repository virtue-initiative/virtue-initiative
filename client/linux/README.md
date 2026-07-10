# Virtue Linux Client

## Commands

- `virtue login`
  - Prompts for email/password.
  - Registers a device with the API and stores `device_id`.
- `virtue logout`
  - Warns that a log event is sent to indicate monitoring was turned off.
  - Clears local auth/device state and disables monitoring.
- `virtue status`
  - Shows login and monitoring state.
- `virtue daemon`
  - Background worker used by systemd.
  - On desktop sessions, it also starts a minimal tray icon with hover status.
- `virtue daemon start`
  - Starts the `virtue.service` user unit through `systemctl --user`.
- `virtue daemon stop`
  - Stops the `virtue.service` user unit through `systemctl --user`.
  - Records an explicit user stop-intent before stopping so the lifecycle event is classified as user-requested.
- `virtue dev upload-log --risk 0.7 [--title ...] [--details ...]`
  - Sends a developer log immediately with the provided risk score.
- `virtue dev add-log --risk 0.7 [--title ...] [--details ...]`
  - Queues a metadata-only developer log into the next encrypted batch.
- `virtue dev add-screenshot --risk 0.7 [--title ...] [--details ...]`
  - Captures a screenshot and queues it into the next encrypted batch.
- `virtue dev upload-batch`
  - Forces the currently queued batch items to upload now.

## Service behavior

The service is installed and auto-started for active desktop users by the package `postinst` script.
Before `virtue login`, monitoring is idle because there is no token/device binding.
After `virtue login`, captures and uploads start automatically.
The tray icon (when available) is started and stopped by the daemon process.
If a tray host is unavailable at daemon startup, monitoring continues without the tray icon.
Linux lifecycle logs include:

- `system_login` when a new login session is observed (OS session/user login, including a fresh boot).
- `system_logout` at the end of an expected-running window (OS session/user logout).
- `suspend_detected` for a suspend interval found retrospectively via boot-vs-monotonic clock divergence.

And lifecycle alerts, fired when the expected login→logout running window doesn't match what was actually observed:

- `unexpected_start` when the process wasn't running during a stretch of awake time between a known login and the first observed heartbeat.
- `unexpected_stop` when the process stopped running before the session's logout.
- `unexpected_gap` for a stretch of awake time (same boot) between two heartbeats with no sample — crash, force-kill-and-restart, or frozen process.
- `user_stop` when the user explicitly quit the monitor (e.g. `virtue daemon stop`) while it was expected to be running.

`system_logout` and `unexpected_stop` are best-effort: abrupt power loss, kernel panic, or very late shutdown network teardown can still prevent immediate delivery.

### Lifecycle Log Distro Support

- Officially supported install path: Debian/Ubuntu-family distributions using the packaged `.deb`.
- Lifecycle logs (`system_login` / `system_logout` / alerts) are supported on Linux distributions that use:
  - `systemd` (for service lifecycle and shutdown-state detection), and
  - procfs with `/proc/sys/kernel/random/boot_id` (startup detection).
- Non-systemd distributions are not currently supported for system lifecycle logs.

Capture/upload timing is file-driven through `~/.config/virtue/config.json`.

Supported keys:

- `api_base_url`
- `capture_interval_seconds` (default `300`, minimum `15`)
- `batch_window_seconds` (default `3600`, minimum `1`)

`virtue status` prints the current CLI-resolved values.

## Runtime Config

Use one `.deb` for both prod and local API. Override values through `~/.config/virtue/config.json`.

```bash
mkdir -p ~/.config/virtue
cat > ~/.config/virtue/config.json <<'EOF'
{
  "api_base_url": "http://localhost:8787",
  "capture_interval_seconds": 120,
  "batch_window_seconds": 900
}
EOF
```

Revert service back to default API:

```bash
rm -f ~/.config/virtue/config.json
```

The core reloads this file during daemon operation, so runtime changes do not require a service restart.

The client uses `XDG_CONFIG_HOME` and `XDG_STATE_HOME` when those variables are set. Otherwise it falls back to `~/.config/virtue/config.json` for config and `~/.local/state/virtue` for mutable state.

## Wayland and X11

`virtue login` runs a capture probe.

- On X11, install `imagemagick` (`import`) or `maim` if capture tools are missing.
- On Wayland, unattended capture support depends on compositor permissions.
  - Recommended for reliability: use an X11 session for monitoring.
  - Alternative: compositor-specific setup that permits `grim` screencopy.

## Build .deb

From the `client/` workspace root:

```bash
./linux/scripts/build-deb.sh
```

The output `.deb` is created under `target/debian/`.

If you prefer `cargo deb`, the crate includes metadata for it, but the script above has no extra Rust tool dependencies.
