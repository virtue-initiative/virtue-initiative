# BePure Windows Client

This directory contains the Windows client implementation:

- `bepure-service.exe`: Windows service that captures screenshots and uploads via `client/core`.
- `bepure-tray.exe`: tray/login app that handles sign-in/sign-out and shared state.
- NSIS packaging scripts to produce a Windows installer executable.

## Layout

- `src/bin/bepure-service.rs`: persistent Windows service entrypoint.
- `src/bin/bepure-tray.rs`: tray app with login/logout UI.
- `src/daemon.rs`: capture, queue, and upload loop using `bepure-client-core`.
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

- `client/windows/dist/bepure-windows-installer-<version>.exe`

## Runtime data locations

The tray app and service share state in:

- `%PROGRAMDATA%\BePure\config\client_state.json`
- `%PROGRAMDATA%\BePure\config\token_store.json`
- `%PROGRAMDATA%\BePure\data\upload_queue.json`
- `%PROGRAMDATA%\BePure\data\service.log`
