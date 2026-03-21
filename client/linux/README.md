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

## Service behavior

The service is installed and auto-started for active desktop users by the package `postinst` script.
Before `virtue login`, monitoring is idle because there is no token/device binding.
After `virtue login`, captures and uploads start automatically.
The tray icon (when available) is started and stopped by the daemon process.
If a tray host is unavailable, monitoring continues and the daemon retries tray registration in the background.
Linux alert logs include:

- `daemon_start` / `daemon_stop_signal` for service process lifecycle.
- `system_startup` when a new kernel boot is detected (via boot-id change).
- `system_shutdown` when stop signal arrives while the host is in systemd `stopping` state.

`system_shutdown` is best-effort: abrupt power loss, kernel panic, or very late shutdown network teardown can still prevent immediate delivery.

### Lifecycle Log Distro Support

- Officially supported install path: Debian/Ubuntu-family distributions using the packaged `.deb`.
- Lifecycle logs (`system_startup` / `system_shutdown`) are supported on Linux distributions that use:
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
