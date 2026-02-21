# BePure Linux Client

## Commands

- `bepure login`
  - Prompts for email/password.
  - Registers a device with the API and stores `device_id`.
  - Enables and starts the user service (`bepure.service`).
- `bepure logout`
  - Warns that a log event is sent to indicate monitoring was turned off.
  - Clears local auth/device state and disables monitoring.
- `bepure status`
  - Shows login and monitoring state.
- `bepure daemon`
  - Background worker used by systemd.

## Service behavior

The service is installed and auto-started for active desktop users by the package `postinst` script.
Before `bepure login`, monitoring is idle because there is no token/device binding.
After `bepure login`, captures and uploads start automatically.

## Wayland and X11

`bepure login` runs a capture probe.

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
