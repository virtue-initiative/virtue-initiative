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
./scripts/build-msix.sh -Version 0.1.0-dev -Profile Debug
```

Expected output:

- `client/windows/dist/virtue-windows-<version>.msix`
- `client/windows/dist/virtue-windows-<version>-setup.zip`

Useful build flags:

- `-Profile Debug|Release` (default: `Debug`)
- `-Clean` (opt-in, only when you need a clean rebuild)
- `-CacheRoot C:\path\to\cache` (default: `%LOCALAPPDATA%\VirtueBuildCache`)
- `-SkipBuild` (reuse existing Rust artifacts and just rebuild/package the WinUI app)
- `-SkipSigning` (build/package the MSIX but leave the artifacts unsigned for external CI signing)
- `-PackagePublisher "CN=Publisher"` (override the MSIX manifest publisher during packaging)
- `-SigningCertificatePath C:\path\to\cert.pfx` (sign with a real code-signing certificate instead of the local dev cert)
- `-SigningTimestampUrl https://timestamp.example` (recommended for verified/release signing)

Signing behavior:

- Local builds without a configured PFX fall back to a self-signed development certificate and emit a `.cer` plus installer helper for sideloading.
- CI can build unsigned artifacts with `-SkipSigning` and then sign the generated `.msix` files externally.
- When a verified PFX is provided, the package manifest publisher is rewritten to match the certificate subject before signing. This must match exactly for MSIX installation to succeed.
- When signing outside the script, set `-PackagePublisher` or `VIRTUE_WINDOWS_PACKAGE_PUBLISHER` to the exact publisher subject that the external signer uses.

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

## Release Signing

GitHub Actions release packaging no longer uses an external signing service. The manifest's `Identity Publisher` (`client/windows/Virtue.WindowsApp/Package.appxmanifest`) must always match the publisher assigned by Partner Center for this app's Store identity, so the workflow lets `build-msix.ps1` fall back to its built-in self-signed development certificate (the same path used for PR/Debug builds) rather than overriding the publisher with an external signing cert's subject. Microsoft Store re-signs the package on ingestion, so a self-signed upload is sufficient for submission; for sideload installs the generated `.cer` in the setup bundle still needs to be trusted on the target machine as before.

## Store Staging Flight Submission

Every push to `staging` runs `scripts/submit-store-flight.ps1` after the release MSIX is
built and published, submitting that same artifact to a pre-created Microsoft Store
"Staging" flight via the classic Store Submission API
(`https://manage.devcenter.microsoft.com/v1.0/my/...`). This is a one-way push: it
creates a new flight submission (deleting any stale, still-`PendingCommit` one left over
from a prior run), replaces the flight's package with the new `.msix`, commits, and polls
Partner Center briefly to catch immediate validation failures before letting the CI job
succeed — Store certification and rollout to flight testers continues asynchronously
afterward, the same way the iOS TestFlight upload step doesn't block on Apple's
processing.

This depends on one-time manual setup in Partner Center / Microsoft Entra ID that this
repo cannot provision on its own:

1. Register a Microsoft Entra ID application (Partner Center can create one directly from
   its "Microsoft Entra applications" management page) and generate a client secret for
   it in the Azure Portal (App registrations → Certificates & secrets).
2. In Partner Center, under Account settings → User management → Microsoft Entra
   applications, associate that app and grant it the Manager role on this developer
   account. (Managing this page requires being signed in as a Partner Center Manager
   who is also a Global Administrator of the Entra tenant.)
3. Look up the app's Store ID in Partner Center (App → App identity).
4. Create the "Staging" flight once in Partner Center (App overview → Package flights →
   New package flight), picking the tester group. Its Flight ID (a GUID) isn't surfaced
   directly in the UI — the reliable way to get it is to call the Store Submission API's
   `listflights` endpoint once the Entra app's credentials work:
   `GET https://manage.devcenter.microsoft.com/v1.0/my/applications/{STORE_APP_ID}/listflights`
5. Add the tenant/client/app/flight IDs as repo **variables** (not secrets — they aren't
   sensitive) and the client secret as a repo **secret**, read by
   `submit-store-flight.ps1` via matching env var names in `client-windows.yml`:
   - `WINSTORE_TENANT_ID` (variable) — Entra tenant ID
   - `WINSTORE_CLIENT_ID` (variable) — Entra app (client) ID
   - `WINSTORE_CLIENT_SECRET` (secret) — Entra app client secret
   - `WINSTORE_APP_ID` (variable) — Partner Center Store ID for this app (e.g. `9NXXXXXXXXXX`)
   - `WINSTORE_FLIGHT_ID` (variable) — GUID of the pre-created "Staging" flight

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
  --version 0.1.0-dev \
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
- The app registers a per-user Scheduled Task (`VirtueResidentWatchdog`, every 1 minute — Task Scheduler's floor for a repeating trigger, confirmed against `schtasks.exe` on this repo's `virtue-win11` VM: a sub-minute `Interval` is rejected as out of range even via an XML task definition) at startup that relaunches `Virtue.WindowsApp.exe` if it isn't running, covering accidental crashes/hangs. The app's existing single-instance redirect makes each periodic relaunch attempt a no-op while the app is already running, so the task doesn't need its own "is it running" check, and the relaunch comes back into the tray quietly (same as the startup task). Tray `Exit` deletes the task first so a deliberate exit isn't resurrected. Windows' Application Recovery and Restart API (`RegisterApplicationRestart`) was tried first but does not automatically relaunch MSIX-packaged apps (confirmed both by community reports and by testing on this repo's `virtue-win11` VM: registration succeeds and WER correctly reports the crash/hang, but no relaunch follows), hence the Scheduled Task instead. See `Virtue.WindowsApp.Core/Interop/RestartWatchdog.cs` and `Virtue.WindowsApp/App.xaml.cs`. The `client/core` late-wakeup tamper threshold (`client/core/SPEC.md` §2) was raised from 1 to 2 minutes so a normal ~1-minute watchdog relaunch doesn't itself trip the tamper alert.
- `client/core` is intentionally unchanged by this architecture; Windows-specific behavior lives under `client/windows/`.

## Runtime Data Locations

The WinUI app and Rust resident monitoring host share state under `%PROGRAMDATA%\Virtue`:

- `%PROGRAMDATA%\Virtue\config\ui_state.json`
- `%PROGRAMDATA%\Virtue\config\token_store.json`
- `%PROGRAMDATA%\Virtue\data\lifecycle_state.json`
- `%PROGRAMDATA%\Virtue\data\logs\virtue.<date>.log` (daily-rotated diagnostic log)

`api_base_url`, `capture_interval_seconds`, and `batch_window_seconds` are no longer
runtime-configurable. They're compile-time constants baked in by `client/core/build.rs`
via `env!()`. To set local dev values, copy `client/.env.example` to `client/.env`
(gitignored) and set `VIRTUE_DEFAULT_API_URL`, `VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS`,
and `VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS` there; real process/CI env vars still take
precedence over `.env`.

The Rust FFI surface exposed to the WinUI app is:

- `virtue_windows_init`
- `virtue_windows_start_monitoring`
- `virtue_windows_stop_monitoring`
- `virtue_windows_get_monitor_status_json`
- `virtue_windows_get_session_status_json`
- `virtue_windows_login`
- `virtue_windows_logout`
- `virtue_windows_free_string`

## Troubleshooting

- No tray icon: confirm the app has been launched once after install and that the Windows startup task for Virtue is enabled.
- Signed in but monitoring inactive: check `%PROGRAMDATA%\Virtue\data\logs\virtue.<date>.log` and the monitor state shown in the settings window.
- Startup did not run: launch Virtue from Start once, then verify the `VirtueTrayStartup` startup entry remains enabled in Windows.
