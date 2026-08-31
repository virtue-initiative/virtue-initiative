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
./scripts/build-msix.sh -Version 0.1.2-dev -Profile Debug
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

## Store Submissions

Two separate Store destinations, driven by branch:

| Branch    | Destination                  | Script                            | Who gets it         |
| --------- | ---------------------------- | --------------------------------- | ------------------- |
| `staging` | "Staging" package flight     | `scripts/submit-store-flight.ps1` | flight testers only |
| `main`    | the app's main Store listing | `scripts/submit-store-app.ps1`    | all Store customers |

Both run after the release MSIX is built and published, submit that same artifact via
the classic Store Submission API (`https://manage.devcenter.microsoft.com/v1.0/my/...`),
and are one-way pushes: each cancels any in-progress submission left over from a prior
run (whatever stage it is at — uncommitted, processing, or in certification — since any
of them block creating a new one), replaces the package, commits, and polls Partner
Center briefly to catch immediate validation failures before letting the CI job succeed.
Store certification and rollout continue asynchronously afterward, the same way the iOS
TestFlight upload step doesn't block on Apple's processing.

The two differ in more than their URLs:

- A **flight** submission is created empty; an **app** submission is created as a clone of
  the last published one, so the store listing, screenshots, age ratings and certification
  notes carry over and only the package is swapped. That clone is PUT back in full, which
  is why `submit-store-app.ps1` serializes it with `ConvertTo-Json -Depth 100` — past its
  depth limit `ConvertTo-Json` silently degrades nested nodes to strings rather than
  erroring, which would rewrite the live listing with garbage.
- The package list field is `applicationPackages`, not `flightPackages`.
- `submit-store-app.ps1` forces `targetPublishMode = Immediate` (and clears any cloned
  `targetPublishDate`) so a merge to `main` actually ships. Pass `-TargetPublishMode Manual`
  to stop after certification and publish by hand instead.

Package versions never collide between the two: `build-msix.ps1` gives stable (`main`)
builds `<VERSION>.0` and dev (`staging`) builds `<major>.<minor>.<GITHUB_RUN_NUMBER>.0`, so
flight packages stay ahead of the main listing as Microsoft's flighting model requires.

### One-time setup

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
     (used only by the `staging` flight submission; the `main` app submission needs just
     the tenant/client/secret/app values above)

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
  --version 0.1.2-dev \
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
- The app registers a per-user Scheduled Task (`VirtueResidentWatchdog`, every 1 minute — Task Scheduler's floor for a _repeating_ trigger: the schema pins `Repetition/Interval` at `minInclusive="PT1M"`, confirmed against `schtasks.exe` on this repo's `virtue-win11` VM, where a sub-minute `Interval` is rejected as out of range even via an XML task definition. That floor binds repetition only — a _one-shot_ trigger can be aimed seconds out, which is what `UpdateRelaunchTask` does after an update; see below) at startup that relaunches `Virtue.WindowsApp.exe` if it isn't running, covering accidental crashes/hangs. The app's existing single-instance redirect makes each periodic relaunch attempt a no-op while the app is already running, so the task doesn't need its own "is it running" check, and the relaunch comes back into the tray quietly (same as the startup task). The task's command is the app execution alias `%LOCALAPPDATA%\Microsoft\WindowsApps\virtue-initiative.exe` (declared as a `uap5:AppExecutionAlias` in `Package.appxmanifest`; resolved by `AppLaunchPath`, unit-tested), **not** `Environment.ProcessPath`. `Environment.ProcessPath` for an MSIX app is version-stamped (`...\WindowsApps\..._0.1.1.0_x64__...\Virtue.WindowsApp.exe`), so a Store update deleted the directory the task pointed at and every later run failed with `0x80070002` (`ERROR_FILE_NOT_FOUND`) — the app the update had just terminated was never brought back, and re-registering on launch couldn't repair it because nothing was left to launch. `explorer.exe shell:AppsFolder\...` is the other version-independent way to start a packaged app but drops command-line arguments, which the quiet-relaunch flag needs. Check a suspect machine with `schtasks /Query /TN VirtueResidentWatchdog /XML` and `Get-ScheduledTaskInfo -TaskName VirtueResidentWatchdog` (`LastTaskResult`). Tray `Exit` deletes the task first so a deliberate exit isn't resurrected. Windows' Application Recovery and Restart API (`RegisterApplicationRestart`) was tried first but does not automatically relaunch MSIX-packaged apps (confirmed both by community reports and by testing on this repo's `virtue-win11` VM: registration succeeds and WER correctly reports the crash/hang, but no relaunch follows), hence the Scheduled Task instead. See `Virtue.WindowsApp.Core/Interop/RestartWatchdog.cs` and `Virtue.WindowsApp/App.xaml.cs`. The `client/core` late-wakeup tamper threshold (`client/core/SPEC.md` §2) was raised from 1 to 2 minutes so a normal ~1-minute watchdog relaunch doesn't itself trip the tamper alert.
- `client/core` is intentionally unchanged by this architecture; Windows-specific behavior lives under `client/windows/`.

## Store Update Handling

`StoreUpdateManager` (`Virtue.WindowsApp/Update/StoreUpdateManager.cs`) polls the Microsoft
Store via `StoreContext.GetAppAndOptionalStorePackageUpdatesAsync`, and if one is found,
downloads/stages it in the background. It lives in `Virtue.WindowsApp` (the WinUI project,
`net8.0-windows10.0.19041.0`) rather than `Virtue.WindowsApp.Core` (plain `net8.0`, no Windows
SDK projection, kept that way so it stays unit-testable and platform-neutral — the same reason
`RestartWatchdog` avoids WinRT), since `StoreContext` is a WinRT API only usable from the
packaged process.

**Silent APIs first, interactive only with a window on screen.** Both
`RequestDownloadStorePackageUpdatesAsync` and `RequestDownloadAndInstallStorePackageUpdatesAsync`
can show an OS consent dialog, and [the install one always
does](https://learn.microsoft.com/en-us/windows/uwp/packaging/self-install-package-updates).
That dialog is owned by the HWND the `StoreContext` was initialized with, which in resident mode
is the tray host's _hidden_ window — so pressing the notice's restart button appeared to do nothing at
all while the install sat waiting on a dialog no one could see. `StoreUpdateManager` therefore
prefers `TrySilentDownloadStorePackageUpdatesAsync` /
`TrySilentDownloadAndInstallStorePackageUpdatesAsync` whenever
`StoreContext.CanSilentlyDownloadStorePackageUpdates` is set (it is, unless the user turned off
"Update apps automatically" in the Store). The `Request*` fallbacks stay for when it isn't, but
the install one is gated behind an explicit `allowInteractive` argument:
`TryInstallStagedUpdateAsync(bool allowInteractive)` returns `UpdateInstallOutcome`
(`Virtue.WindowsApp.Core/Interop/UpdateInstallOutcome.cs`) —

- `Installed` — done; the OS normally terminates the process for the package swap.
- `NeedsUserInteraction` — consent is required and the caller asked for silent. **Nothing was
  attempted and the update stays staged.** `App` responds by showing the main window and
  retrying once with `allowInteractive: true`, so the dialog has a real, visible owner.
- `Failed` — attempted and didn't complete; the staged flag is cleared and the check loop
  retries on its backoff.

**The `StoreContext` needs an owner window.** In a packaged _desktop_ app (as opposed to a UWP
one), the context returned by `StoreContext.GetDefault()` must be associated with an owner HWND
via the `IInitializeWithWindow` interop — `WinRT.Interop.InitializeWithWindow.Initialize(context,
hwnd)` — before any Store call that _can_ show UI. `GetAppAndOptionalStorePackageUpdatesAsync`
doesn't need one, but the `Request*` calls do; without it they fail with
`ERROR_INVALID_WINDOW_HANDLE` (`0x80070578`), which is what every build before this one did — so
auto-update never actually worked, and neither did the "update ready" UI downstream of it. The
app normally runs resident with no `MainWindow` at all, so the owner is the tray host's hidden
top-level window, surfaced as `ITrayIconHost.WindowHandle` (a raw `IntPtr`, so `Core` keeps its
no-WinRT constraint): the one HWND guaranteed to exist for the whole process lifetime.

**Monitoring is never stopped for an update.** The daemon keeps running right up until the OS
terminates the process for the package swap. Stopping it first (which earlier builds did, via
`StopMonitoringForOsSessionEnd`) flipped the window and tray tooltip to "Monitoring stopped" for
the whole install — indefinitely, when the install was blocked on an invisible dialog. There is
no data-corruption risk in being terminated mid-tick: `DaemonState` is persisted after every
tick and uploads use persisted backoff, so this is exactly the shape of a crash-recovery
relaunch, which `RestartWatchdog` and CORE-002's late-wakeup budget already cover. `client/mac`'s
Sparkle updater (`client/mac/app/Sources/UpdateController.swift`) made the same call
deliberately. `HandleSessionLogoff`, `HandleSystemShutdown` and tray `Exit` keep their stop
calls — those are real session ends, not updates.

Check cadence comes from `StoreUpdateRetryPolicy`
(`Virtue.WindowsApp.Core/Interop/StoreUpdateRetryPolicy.cs`, unit-tested): 4 hours after a clean
check, but a failed one retries after 5 minutes and doubles up to an hour, so a transient (or
newly-introduced) failure no longer costs a full 4-hour cycle. The loop itself wakes on a 5-second
slice and tracks its own next-check time, rather than sleeping one long delay, so the debug
sentinel below and the manual re-check are picked up promptly and all staging stays on one
thread. The status card's **Check for updates** button calls `RequestCheckNow()`, which releases
a semaphore that slice waits on — short-circuiting the cadence is its _only_ effect. Feedback
appears next to it via `SessionViewModel.UpdateCheckStatusText` ("Checking for updates..." →
"No updates found." / "Update found; downloading..." / "Update check failed."); see the status-line
rules below.

Once an update is staged, restart is driven by one shared decision method,
`App.EvaluateUpdateRestart()`, called from four places: immediately when the update stages, from
a 1-minute countdown timer, from `MainWindow.Hidden` — an event raised at the end of every
`HideToTray()` call (the X-button close, the tray-Exit confirmation's cancel-path re-hide, and
the update flow's own hide all funnel through it uniformly), so the restart fires the instant the
window closes instead of waiting for the next timer tick — and from `ShowMainWindow()`, so the
notice card gets a live countdown immediately rather than the generic "will install soon" text.
Its whole body is marshalled onto the UI thread, since it reads `MainWindow.IsVisibleToUser`
(written there) but is called from the countdown pool thread.

If the window is hidden and no login/logout is in progress (`SessionViewModel.IsBusy`), it
restarts right away. If the window is visible, it instead updates an in-window notice card with a
countdown (`UpdateRestartPolicy.GetDeadlineUtc`/`FormatCountdown`,
`Virtue.WindowsApp.Core/Interop/UpdateRestartPolicy.cs`, unit-tested) reading "Virtue will restart
to update in ...", with a "Restart now to update" button below it. Once the 6-hour deferral cap is
reached (`UpdateRestartPolicy.ShouldForceRestart`), `EvaluateUpdateRestart()` hides the window
itself, which re-raises `Hidden` and re-enters the method — now that the window reports hidden, it
takes the immediate-restart branch. The notice button calls the manual-restart handler, bypassing
the busy/deadline check as an explicit user request and passing `allowInteractive: true`. An
`Interlocked`-guarded flag in `InstallUpdateAndRestartAsync` ensures only the first of these
concurrent triggers actually proceeds. `RestartWatchdog` is deliberately left registered through
all of this (unlike tray Exit) so its per-minute poll relaunches the updated build, and
the released `AppLifecycleInstance` key means the relaunched process becomes the new primary
instance with no extra code, exactly like a crash-recovery relaunch.

#### Coming back within seconds, not within a minute

The watchdog poll alone leaves the app gone for anywhere from 0 to 60 seconds after an update.
`UpdateRelaunchTask` (`Virtue.WindowsApp.Core/Interop/UpdateRelaunchTask.cs`, unit-tested) closes
that to a measured ~15s: immediately before each install attempt, `App.ScheduleUpdateRelaunch()`
registers a **one-shot** `VirtueUpdateRelaunch` task pointing at the same `AppLaunchPath`
execution alias the watchdog uses.

The app cannot restart itself here. The OS terminates the process for the package swap, so there
is no "after" in which to run code, and every process carrying the package identity — including
any helper the app spawned, since child processes inherit that identity — is terminated with it.
The relaunch has to be owned by something outside the package, and the scheduler service is.
`Microsoft.Windows.AppLifecycle.AppInstance.Restart` specifically cannot serve: [its design
spec](https://github.com/microsoft/WindowsAppSDK/blob/main/specs/AppLifecycle/Restart/restartApi.markdown)
has the helper agent take "the executable path of the application" and `CreateProcess` it, which
for an MSIX app is the version-stamped `WindowsApps` directory the update deletes — the same
`0x80070002` trap that `AppLaunchPath` exists to avoid — and it has to be called from a live
process, which we no longer are once the swap begins.

The trigger is a `TimeTrigger` with a seconds-precision `StartBoundary`, registered via
`schtasks /Create /XML` (the `/TR` shorthand cannot express it: `/ST` is documented as `HH:mm`,
with no seconds field, so even `/SC ONCE` is stuck on minute boundaries). A `RegistrationTrigger`
with a sub-minute `<Delay>` is the more natural fit — it is relative, so it sidesteps clock skew
— and its schema imposes no minimum, but on the VM it **never fires at all** when registered
through `schtasks /Create /XML`: the task registers successfully, then sits at
`SCHED_S_TASK_HAS_NOT_RUN` (`267011`) indefinitely. The `TimeTrigger` shape was measured firing at
15.86s for a 15s delay, with `DeleteExpiredTaskAfter` collecting the task afterwards on schedule.
`Principal` carries no `UserId` — `schtasks` resolves the invoking account itself (the registered
task reports `Run As User: help` on the VM), which also keeps a Windows-only `WindowsIdentity`
SID lookup out of the plain-`net8.0` `Core` project.

This is an optimization and never a guarantee, which is why the watchdog stays registered
throughout. If the one shot fires while the swap is still in flight, the launch fails and the
shot is spent; the watchdog still picks the app up within a minute, exactly as before this
existed. Every failure degrades to that. The task is cancelled on the paths where it would
otherwise fire into a running app: at startup (`RegisterWatchdog()`), on tray Exit alongside
`RestartWatchdog.Unregister()`, and in `ClearStagedUpdateAfterFailedInstall()`. It is also
re-armed before the interactive install retry, since the first shot has very likely already
fired while the consent dialog was up.

Pressing the notice's button also sets `RelaunchWindowFlag`
(`%PROGRAMDATA%\Virtue\show-window-on-next-launch`, unit-tested) and flips the card to
"Installing the update. Virtue will close and restart itself within a minute." with the button
disabled. The flag is consumed by the next launch, which then shows the main window instead of
coming back quietly into the tray: the user asked for a restart, so the process vanishing with
nothing visibly returning is the wrong feedback. It is consumed unconditionally at startup, and
also by `ClearStagedUpdateAfterFailedInstall()` (where the process survived, so no launch is
coming), so a stale flag can't pop a window at some unrelated later launch. An automatic
window-closed update sets no flag and stays quiet.

The status line beside "Check for updates" reports every terminal outcome — "No updates found.",
"Update found; downloading...", "Update downloaded.", "Update check failed.", "Update install did
not complete; it will be retried." — and the button short-circuits to "Update already downloaded."
when one is staged, since the check loop skips checks entirely in that state and the line would
otherwise sit on "Checking for updates..." forever. Nothing sets it back to empty: a blank line
after a click is indistinguishable from a hang.

There is deliberately **no tray "Restart to Update" item**. With the window closed —
the resident app's normal state — `EvaluateUpdateRestart()` restarts _immediately_ on staging,
so such an item would appear only in the instant before the process exits (or in the narrow
window where a login/logout is in flight). The window-open case is already served by the
in-window card, and dropping the item also removes the one caller that rebuilt the tray HMENU
from a threadpool thread — HMENUs are USER objects owned by their creating thread, so
`SetForceCaptureAvailable` is likewise marshalled through the UI dispatcher.

The app **only exits when the install actually succeeded**. Exiting on a failed install would
spin — watchdog relaunch within a minute, re-stage, exit again — so anything other than
`Installed` runs `ClearStagedUpdateAfterFailedInstall()` instead: it cancels the countdown timer,
clears `_updateStagedAtUtc`, calls `SessionViewModel.NotifyUpdateUnstaged()` (which drops the
notice card, the countdown text and the tray tooltip's " (update ready)" suffix) and releases the
restart guard. Skipping any of that is what previously wedged the app: the card stayed up with a
frozen countdown over a dead button, and `EvaluateUpdateRestart()` early-returned forever, so
closing the window no longer updated either. The check loop re-stages on its own backoff, which
rebuilds all of it through `OnUpdateStaged`.

### Debug sentinel (simulating a staged update)

Reaching any of the above normally requires publishing a newer package to a Store flight, so
there is a developer-only shortcut. Write a file at
`%PROGRAMDATA%\Virtue\debug-stage-update` — `DebugUpdateSentinel`
(`Virtue.WindowsApp.Core/Interop/DebugUpdateSentinel.cs`, unit-tested) — and within ~5 seconds
the poll slice consumes it (reading, then deleting it) and raises the _same_ `UpdateStaged`
event a real download would, so the countdown, the window-closed auto-restart and the button all
run for real. `TryInstallStagedUpdateAsync` then returns `Installed` without touching the Store,
the app exits, and `UpdateRelaunchTask` brings it back ~15 seconds later (with `RestartWatchdog`
behind it within a minute), so the whole relaunch path is exercised too.

The file's contents may be a short duration (`5m`, `90s`, `2h`; a bare number is minutes) that
overrides `UpdateRestartPolicy.DeferralCap` for that simulated update, making the forced-restart
path reachable in minutes instead of six hours. Empty or unparseable contents still stage the
update, just with the normal cap.

```powershell
# Window open: notice card with a 2-minute countdown, then a forced restart.
echo 2m > C:\ProgramData\Virtue\debug-stage-update
```

### Observability

`%PROGRAMDATA%\Virtue\ui-startup.log` is the only observability channel on a Store install, so
the whole check lifecycle is logged there — including the zero-updates case, which used to be
silent and made "checked, nothing available" indistinguishable from "never checked". A healthy
auto-update reads roughly:

```
Store update check starting (owner window initialized=True).
Store update check found 1 update(s).
Store update silent download started.
Store update download finished (OverallState=Completed).
Store update staged.
Store update install re-fetch found 1 update(s).
Store update silent install started.
Store update install finished (OverallState=Completed).
Update install call returned (outcome=Installed).
```

and the fallback, when the user has turned off the Store's automatic updates:

```
Store update install re-fetch found 1 update(s).
Store update install needs user consent; leaving it staged for an interactive retry.
Update install call returned (outcome=NeedsUserInteraction).
Update install needs user consent; showing the main window and retrying interactively.
Store update interactive install started (silent install unavailable).
Store update install finished (OverallState=Completed).
Interactive update install call returned (outcome=Installed).
```

Failures are logged as `Store update check/download failed: <Type>: <message> (0x........)` —
the HRESULT is included deliberately, since it is what identifies a Store failure (`0x80070578`
being the missing-owner-window one above).

No `client/core`/SPEC.md changes were needed for this feature — the fix was to stop calling an
existing stop function, not to add one.

## Runtime Data Locations

The WinUI app and Rust resident monitoring host share state under `%PROGRAMDATA%\Virtue`:

- `%PROGRAMDATA%\Virtue\config\ui_state.json`
- `%PROGRAMDATA%\Virtue\config\token_store.json`
- `%PROGRAMDATA%\Virtue\data\lifecycle_state.json`
- `%PROGRAMDATA%\Virtue\data\logs\virtue.<date>.log` (daily-rotated diagnostic log)

`api_base_url`, `capture_interval_seconds`, and `batch_window_seconds` are no longer
runtime-configurable. They're compile-time constants baked in by `client/core/build.rs`
via `env!()`. To set local dev values, copy `.env.example` (repo root) to `.env`
(gitignored) and set `VIRTUE_DEFAULT_API_URL`, `VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS`,
and `VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS` there; real process/CI env vars still take
precedence over `.env`, which in turn takes precedence over `~/.config/virtue-dev.env`.

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
