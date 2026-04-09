# Developer overrides

To overwrite the default settings for development create a config file with the
following content.

```json
{
  "api_base_url": "http://localhost:8787",
  "capture_interval_seconds": 15,
  "batch_window_seconds": 60
}
```

Use these settings to point a client at a local API or shorten capture timing while
testing.

## Where to set them

- Android: login screen under `Runtime overrides (optional)` or `context.filesDir/core-config/config.json`.
- iOS: app runtime override UI. or `<app group container>/virtue/config/config.json`
- Linux: `~/.config/virtue/config.json`.
- macOS: `~/Library/Application Support/virtue/config.json`.
- Windows: `%PROGRAMDATA%\\Virtue\\config\\config.json`.

## Notes

- Android emulators should use `http://10.0.2.2:8787`, not `http://localhost:8787`.
- Linux, macOS, and Windows usually need a service restart after editing overrides.
- When switching between prod and local APIs, log out and log back in again.
