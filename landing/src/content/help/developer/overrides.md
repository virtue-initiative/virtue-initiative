# Developer Overrides

The main developer overrides are:

- `VIRTUE_BASE_API_URL`
- `VIRTUE_CAPTURE_INTERVAL_SECONDS`
- `VIRTUE_BATCH_WINDOW_SECONDS`

Use them to point a client at a local API or shorten capture timing while
testing.

## Where to set them

- Android: login screen under `Runtime overrides (optional)`.
- iOS: app runtime override UI.
- Linux: `systemctl --user edit virtue.service` or `~/.config/systemd/user/virtue.service.d/override.conf`.
- macOS: `~/Library/Application Support/virtue/service.dev.env`.
- Windows: `%PROGRAMDATA%\\Virtue\\config\\service.dev.env`.

## Notes

- Android emulators should use `http://10.0.2.2:8787`, not `http://localhost:8787`.
- Linux, macOS, and Windows usually need a service restart after editing overrides.
- When switching between prod and local APIs, log out and log back in again.
- Keep intervals realistic; extremely small values create noisy test results.
