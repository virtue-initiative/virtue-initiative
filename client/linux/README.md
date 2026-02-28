# Virtue Linux Client

## Development

Copy `.env.example` to `.env`

## Commands

- `virtue login`
  - Prompts for email/password.
  - Registers a device with the API and stores `device_id`.
  - Enables and starts the user service (`virtue.service`).
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

Capture interval can be overridden with environment variable `BEPURE_CAPTURE_INTERVAL_SECONDS`.
Minimum interval is `15` seconds.

## Local API Override

Use one `.deb` for both prod and local API. Override the base URL with `BEPURE_BASE_API_URL`.

Set override for the background service:

```bash
mkdir -p ~/.config/systemd/user/virtue.service.d
cat > ~/.config/systemd/user/virtue.service.d/override.conf <<'EOF'
[Service]
Environment=BEPURE_BASE_API_URL=http://localhost:8787
EOF
systemctl --user daemon-reload
systemctl --user restart virtue.service
```

Run one-off CLI commands against local API:

```bash
BEPURE_BASE_API_URL=http://localhost:8787 virtue login
```

Revert service back to default API:

```bash
rm -f ~/.config/systemd/user/virtue.service.d/override.conf
systemctl --user daemon-reload
systemctl --user restart virtue.service
```

When switching API environments, run `virtue logout` then `virtue login` again.

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
