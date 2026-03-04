# Virtue Windows Client

This directory contains the Windows client implementation:

- `virtue-service.exe`: Windows service that captures screenshots and uploads via `client/core`.
- `virtue-tray.exe`: tray/login app that handles sign-in/sign-out and shared state.
- NSIS packaging scripts to produce a Windows installer executable.

## Layout

- `src/bin/virtue-service.rs`: persistent Windows service entrypoint.
- `src/bin/virtue-tray.rs`: tray app with login/logout UI.
- `src/daemon.rs`: capture, queue, and upload loop using `virtue-client-core`.
- `src/capture.rs`: Windows GDI screenshot capture.
- `packaging/nsis/installer.nsi`: installer definition.
- `scripts/build-installer.ps1`: Windows host build + packaging script.
- `scripts/build-installer.sh`: WSL wrapper for the PowerShell build script.

## Prerequisites (Windows host)

- Rust MSVC toolchain (`stable-x86_64-pc-windows-msvc`)
- Visual Studio Build Tools (C++ workload)
- NSIS (`makensis.exe`)

## Build Installer

From WSL:

```bash
cd /home/jeff/code/bepurev2/client/windows
./scripts/build-installer.sh 0.1.0
```

Or from Windows PowerShell:

```powershell
cd \\wsl$\Ubuntu\home\jeff\code\bepurev2\client\windows
.\scripts\build-installer.ps1 -Version 0.1.0
```

Expected output:

- `client/windows/dist/virtue-windows-installer-<version>.exe`

## Custom API base URL

The Windows client reads `VIRTUE_BASE_API_URL` (same as other clients).

PowerShell example:

```powershell
$env:VIRTUE_BASE_API_URL = "https://your-api.example.com"
```

For background capture, set it as a machine-level environment variable and log out/in.
Capture and tray are launched from machine startup Run keys (`HKLM`):

- `VirtueCapture` starts hidden capture (`virtue-service.exe --console` via `wscript`).
- `VirtueTray` starts the tray app.
  The installer also creates all-users Startup-folder shortcuts as a fallback.

## Capture interval override

The Windows client supports `VIRTUE_CAPTURE_INTERVAL_SECONDS`.
Minimum interval is `15` seconds.

## Batch window override

The Windows client supports `VIRTUE_BATCH_WINDOW_SECONDS`.
Minimum window is `1` second.

## Service dev override file

For local/dev service overrides (similar to Linux service env overrides), place a file at:

- `%PROGRAMDATA%\Virtue\config\service.dev.env`

Supported keys:

- `VIRTUE_BASE_API_URL`
- `VIRTUE_CAPTURE_INTERVAL_SECONDS`
- `VIRTUE_BATCH_WINDOW_SECONDS`

Example:

```env
VIRTUE_BASE_API_URL=http://localhost:8787
VIRTUE_CAPTURE_INTERVAL_SECONDS=120
VIRTUE_BATCH_WINDOW_SECONDS=900
```

After editing the file, log out/in (or restart the `virtue-service.exe --console` process).

## Runtime data locations

The tray app and service share state in:

- `%PROGRAMDATA%\Virtue\config\client_state.json`
- `%PROGRAMDATA%\Virtue\config\token_store.json`
- `%PROGRAMDATA%\Virtue\data\batch_buffer.json`
- `%PROGRAMDATA%\Virtue\data\service.log`

Installer upgrades keep these files intact. On first upgrade from older `BePure`
installs, the installer migrates compatible state files from `%PROGRAMDATA%\BePure`
if the corresponding `%PROGRAMDATA%\Virtue` file is missing.

When API/network is unavailable, the daemon keeps capturing locally and retries
token/settings/upload operations periodically (every ~30 seconds for control-plane calls).
While the daemon is running, it also periodically ensures the tray process is running.
