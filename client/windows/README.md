# Virtue Windows Client

The Windows client now lives under one directory:

- `client/windows/`: Rust backend, native FFI layer, resident monitoring host, WinUI 3 desktop app, and packaging scripts.

The supported runtime is a single WinUI 3 desktop app that starts in the signed-in user session, owns the tray icon, and hosts monitoring in-process through the Rust DLL. The supported install artifact is an MSIX package plus setup bundle built from the WinUI app and Rust DLL payload.

## Layout

- `src/resident_monitor.rs`: in-process monitoring loop used by the resident app.
- `src/ffi.rs`: C ABI exported for the WinUI app (`virtue_windows_*` functions).
- `src/session.rs`: shared auth/session logic reused by the FFI layer.
- `scripts/build-msix.ps1`: Windows-host build and packaging script for the WinUI app + Rust payload.
- `scripts/remote-windows-build.sh`: Linux-driven SSH build loop for smoke checks and MSIX packaging.
- `Virtue.WindowsApp/`: WinUI 3 desktop app.
- `Virtue.WindowsApp.Core/`: managed interop client, tray controller, and session view model.
- `Virtue.WindowsApp.Tests/`: managed tests for the non-UI WinUI support layer.

## Prerequisites (Windows host)

If you are on Linux, use the VM-based workflow in `Linux-Driven Remote Windows Loop` below instead of trying to build the Windows app locally. The supported Linux path is to drive the `win11` VM over SSH.

- Rust MSVC toolchain (`stable-x86_64-pc-windows-msvc`)
- .NET 8 SDK
- Visual Studio Build Tools with:
  - MSBuild
  - desktop C++ build tools
  - managed desktop build tools
  - Windows 10 SDK 19041+
- Optional: `sccache`

The bootstrap script in `scripts/bootstrap-win11-build-vm.ps1` installs the recommended toolchain for the build VM.

## Build MSIX

If the current OS is Linux, skip this section and use the VM instructions below. Local build commands in this section are for Windows hosts only.

From WSL:

```bash
cd /home/jeff/code/virtue-initiative/client/windows
./scripts/build-msix.sh -Version 0.0.5-dev -Profile Debug
```

Expected output:

- `client/windows/dist/virtue-windows-<version>.msix`
- `client/windows/dist/virtue-windows-<version>-setup.zip`

Useful build flags:

- `-Profile Debug|Release` (default: `Debug`)
- `-Clean` (opt-in, only when you need a clean rebuild)
- `-CacheRoot C:\path\to\cache` (default: `%LOCALAPPDATA%\VirtueBuildCache`)
- `-SkipBuild` (reuse existing Rust artifacts and just rebuild/package the WinUI app)

## Windows CI smoke checks (local, cached)

If the current OS is Linux, use the remote `win11` VM smoke-check flow below. These local commands are for Windows hosts only.

The CI-equivalent smoke checks now cover both Rust and the managed WinUI support layer:

```powershell
cd C:\path\to\virtue-initiative\client
cargo build --target x86_64-pc-windows-msvc -p virtue-core
cargo build --target x86_64-pc-windows-msvc -p virtue-windows
cargo clippy --target x86_64-pc-windows-msvc -p virtue-core --all-targets -- -D warnings
cargo clippy --target x86_64-pc-windows-msvc -p virtue-windows --all-targets -- -D warnings
dotnet restore .\windows\Virtue.WindowsApp\Virtue.WindowsApp.csproj
dotnet build .\windows\Virtue.WindowsApp.Core\Virtue.WindowsApp.Core.csproj -c Debug
dotnet test .\windows\Virtue.WindowsApp.Tests\Virtue.WindowsApp.Tests.csproj -c Debug
dotnet build .\windows\Virtue.WindowsApp\Virtue.WindowsApp.csproj -c Debug -p:Platform=x64 -p:AppxPackageSigningEnabled=false -p:GenerateAppxPackageOnBuild=false
```

The packaging script also runs the managed tests before producing the MSIX artifacts.

## Linux-Driven Remote Windows Loop

If the current OS is Linux, always use this VM workflow for Windows builds and validation.

Prereqs:

- OpenSSH server enabled on the Windows VM
- `ssh`/`scp` available on Linux host
- SSH alias configured (for example `win11`)

If you are rebuilding from scratch, follow the full guide first:

- [VM_SETUP.md](./VM_SETUP.md)

Run remote smoke checks from Linux:

```bash
./client/windows/scripts/remote-windows-build.sh \
  --build-host win11 \
  --mode smoke
```

Build the Windows MSIX package from Linux:

```bash
./client/windows/scripts/remote-windows-build.sh \
  --build-host win11 \
  --mode msix \
  --version 0.0.5-dev \
  --profile Debug
```

Remote logs are always written locally under:

- `client/windows/dist/remote-logs/`

## Runtime Behavior

- The packaged startup task launches `Virtue.WindowsApp.exe` at user logon in resident mode.
- Resident mode creates the tray icon and starts the Rust monitoring host without opening a window.
- Launching the app from Start reuses the resident instance and opens the settings/login window.
- Closing the settings window hides it back to the tray.
- Tray `Exit` asks for confirmation before stopping active monitoring, records an explicit user stop, stops resident monitoring, and exits the app without logging the device out.
- `client/core` is intentionally unchanged by this architecture; Windows-specific behavior lives under `client/windows/`.

## Runtime Data Locations

The WinUI app and Rust resident monitoring host share state under `%PROGRAMDATA%\Virtue`:

- `%PROGRAMDATA%\Virtue\config\config.json`
- `%PROGRAMDATA%\Virtue\config\ui_state.json`
- `%PROGRAMDATA%\Virtue\config\token_store.json`
- `%PROGRAMDATA%\Virtue\data\audit.jsonl`
- `%PROGRAMDATA%\Virtue\data\lifecycle_state.json`
- `%PROGRAMDATA%\Virtue\data\service.log`

Runtime config overrides continue to support:

- `apiBaseUrl`
- `captureIntervalSeconds`
- `batchWindowSeconds`

The Rust FFI surface exposed to the WinUI app is:

- `virtue_windows_init`
- `virtue_windows_start_monitoring`
- `virtue_windows_stop_monitoring`
- `virtue_windows_get_monitor_status_json`
- `virtue_windows_get_session_status_json`
- `virtue_windows_login`
- `virtue_windows_logout`
- `virtue_windows_get_runtime_config_json`
- `virtue_windows_set_runtime_config_json`
- `virtue_windows_free_string`

## Troubleshooting

- No tray icon: confirm the app has been launched once after install and that the Windows startup task for Virtue is enabled.
- Signed in but monitoring inactive: check `%PROGRAMDATA%\Virtue\data\service.log` and the monitor state shown in the settings window.
- Startup did not run: launch Virtue from Start once, then verify the `VirtueTrayStartup` startup entry remains enabled in Windows.
