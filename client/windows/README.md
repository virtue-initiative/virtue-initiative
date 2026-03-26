# Virtue Windows Client

This directory contains the Windows client implementation:

- `virtue-service.exe`: shared executable with two modes:
  - lifecycle service mode (`--mode lifecycle`) for startup/shutdown/session logs
  - user-session capture mode (`--mode capture --console`) for screenshots/uploads
- `virtue-tray.exe`: tray/login app that handles sign-in/sign-out and shared state.
- NSIS packaging scripts to produce a Windows installer executable.

Windows alert logs include:

- `service_start` / `service_stop` for lifecycle service transitions.
- `daemon_start` / `daemon_stop_signal` for service process lifecycle.
- `system_startup` when a new Windows boot is detected (via boot-time change).
- `system_shutdown` when an explicit Windows shutdown control/signal is observed.
- `session_login` / `session_logout` from Windows session-change notifications.

If Windows terminates the capture process too late in shutdown to flush logs, the
next boot emits recovered `daemon_stop_signal`/`system_shutdown` events with
`detected_by=next_boot_recovery`.

`system_shutdown` is best-effort: abrupt power loss, forced termination, or late-shutdown networking can still prevent immediate delivery.

## Layout

- `src/bin/virtue-service.rs`: mode switch + lifecycle service + capture console entrypoint.
- `src/bin/virtue-tray.rs`: tray app with login/logout UI.
- `src/daemon.rs`: lifecycle logging daemon (startup/shutdown/session/service events).
- `src/capture_daemon.rs`: screenshot capture/upload daemon using `virtue-core`.
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
cd /home/jeff/code/virtue-initiative/client/windows
./scripts/build-installer.sh -Version 0.0.1 -Profile Debug
```

Or from Windows PowerShell:

```powershell
cd C:\path\to\virtue-initiative\client\windows
.\scripts\build-installer.ps1 -Version 0.0.1 -Profile Debug
```

Expected output:

- `client/windows/dist/virtue-windows-installer-<version>.exe`

Useful installer build flags:

- `-Profile Debug|Release` (default: `Debug`)
- `-Clean` (opt-in, only when you need a clean rebuild)
- `-CacheRoot C:\path\to\cache` (default: `%LOCALAPPDATA%\VirtueBuildCache`)

## Windows CI smoke checks (local, cached)

The CI-equivalent smoke checks run:

- `cargo build -p virtue-core`
- `cargo build -p virtue-windows`
- `cargo clippy -p virtue-core --all-targets -- -D warnings`
- `cargo clippy -p virtue-windows --all-targets -- -D warnings`

On Windows host:

```powershell
cd C:\path\to\virtue-initiative\client
cargo build -p virtue-core
cargo build -p virtue-windows
cargo clippy -p virtue-core --all-targets -- -D warnings
cargo clippy -p virtue-windows --all-targets -- -D warnings
```

This uses persistent cache dirs under `%LOCALAPPDATA%\VirtueBuildCache` and
enables `sccache` automatically when available.

## Linux-driven remote Windows loop

Prereqs:

- OpenSSH server enabled on the Windows VM
- `ssh`/`scp` available on Linux host
- SSH alias configured (example: `win11`)

If you are rebuilding from scratch, follow the full guide first:

- [VM_SETUP.md](./VM_SETUP.md)

Run CI smoke checks from Linux:

```bash
./client/windows/scripts/remote-windows-build.sh \
  --build-host win11 \
  --mode smoke
```

Run an installer build from Linux (artifact stays on Windows by default):

```bash
./client/windows/scripts/remote-windows-build.sh \
  --build-host win11 \
  --mode installer \
  --version 0.0.1-dev \
  --profile Debug
```

Optional: copy installer back to Linux if needed:

```bash
./client/windows/scripts/remote-windows-build.sh \
  --build-host win11 \
  --mode installer \
  --version 0.0.1-dev \
  --profile Debug \
  --copy-installer-to-linux
```

Each remote run writes a full local log file under:

- `client/windows/dist/remote-logs/`

## libvirt / virt-manager VM setup

Create a `win11` VM from ISO (manual install flow):

```bash
virt-install \
  --name win11 \
  --memory 4608 \
  --vcpus 3 \
  --cpu host-passthrough \
  --os-variant win11 \
  --machine q35 \
  --disk size=120,bus=virtio \
  --cdrom ~/isos/Win11_English_x64.iso \
  --disk path=~/isos/virtio-win.iso,device=cdrom \
  --network network=default,model=virtio \
  --graphics spice \
  --video virtio \
  --boot uefi \
  --noautoconsole
```

Start VM:

```bash
virsh start win11
```

Stop VM:

```bash
virsh shutdown win11
```

Force-stop VM if needed:

```bash
virsh destroy win11
```

Delete VM and disk:

```bash
virsh undefine win11 --nvram --remove-all-storage
```

Adjust CPU/RAM from `virt-manager` UI (`Open` VM -> `Show virtual hardware details`).

Bootstrap script for inside-Windows setup (OpenSSH + toolchain + cache paths):

- `client/windows/scripts/bootstrap-win11-build-vm.ps1`

## Custom API base URL

The Windows client reads `VIRTUE_BASE_API_URL` (same as other clients).

PowerShell example:

```powershell
$env:VIRTUE_BASE_API_URL = "https://your-api.example.com"
```

For background capture, set it as a machine-level environment variable and log out/in.
Lifecycle logging runs from an auto-start Windows service (`VirtueLifecycleService`).
Capture and tray are launched from machine startup Run keys (`HKLM`):

- `VirtueCapture` starts hidden capture (`virtue-service.exe --mode capture --console` via `wscript`).
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

After editing the file, restart the lifecycle service and/or capture process:

- `sc stop VirtueLifecycleService && sc start VirtueLifecycleService`
- or log out/in to restart capture startup entries.

## Runtime data locations

The tray app and service share state in:

- `%PROGRAMDATA%\Virtue\config\client_state.json`
- `%PROGRAMDATA%\Virtue\config\token_store.json`
- `%PROGRAMDATA%\Virtue\data\batch_buffer.json`
- `%PROGRAMDATA%\Virtue\data\lifecycle_state.json`
- `%PROGRAMDATA%\Virtue\data\service.log`

Installer upgrades keep these files intact. On first upgrade from older `BePure`
installs, the installer migrates compatible state files from `%PROGRAMDATA%\BePure`
if the corresponding `%PROGRAMDATA%\Virtue` file is missing.

When API/network is unavailable, the daemon keeps capturing locally and retries
token/settings/upload operations periodically (every ~30 seconds for control-plane calls).
While the daemon is running, it also periodically ensures the tray process is running.
