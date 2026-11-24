# Windows client design

Why the Windows-specific parts work the way they do. The behavior itself is specified in
`client/SPEC.md` (CLIENT-027 to CLIENT-029). For building and running, see
[README.md](README.md).

## Process model

There is one process. `Virtue.WindowsApp.exe` (WinUI 3) starts at sign-in through the package's
`VirtueTrayStartup` startup task, owns the tray icon, and loads the Rust library
(the `virtue_windows` cdylib) over P/Invoke. `src/resident_monitor.rs` builds one `Daemon` and runs its
loop on a background thread inside that process. The C ABI the app calls is in `src/ffi.rs`
(`virtue_windows_*`).

- `Virtue.WindowsApp/`: the WinUI app: windows, tray, Store updates. It can use WinRT.
- `Virtue.WindowsApp.Core/`: interop, tray controller, session view model and the update and
  watchdog policies. It targets plain `net8.0` with no Windows SDK projection, so
  `Virtue.WindowsApp.Tests/` can unit-test it. Keep WinRT out of it.

Launching the app from Start reuses the running instance and opens the window. Closing the
window hides it to the tray. Tray Exit asks first, records a user stop, stops monitoring and
exits without signing out.

## Watchdog

The app registers a per-user Scheduled Task, `VirtueResidentWatchdog`, at startup. Every minute
it relaunches the app if it isn't running, which covers crashes and hangs. The app's
single-instance redirect turns each relaunch into a no-op while it is already running.

The task runs the app's exe directly and nothing else. An MSIX uninstall runs no app code, so the
task is the one thing that outlives the package, and it cleans itself up by expiring (CLIENT-014,
CLIENT-028): its trigger ends a day after registration and `DeleteExpiredTaskAfter` has Task
Scheduler delete it then. The app registers it again at every launch and every 6 hours while it
runs, so it never expires while Virtue is in use. After an uninstall it fails quietly (the exe is
gone) for up to a day and then disappears. Tray Exit deletes it.

The device state needs no cleanup, because it lives in the package's own `LocalState\Virtue`,
which Windows deletes at uninstall (CLIENT-002). See [Files on a running machine](README.md#files-on-a-running-machine).

- **No script.** For a while (#885, never released) the task was a hidden PowerShell script under
  `conhost.exe --headless` that deleted `%ProgramData%\Virtue` and unregistered itself after an
  uninstall. Bitdefender quarantined the task and then blocked the script as malware (#920): a
  hidden console, inline PowerShell, file deletion and self-removal are the shape of malware
  persistence. Moving the state into the package removed the need for any of it.
- **Expiry leaves a known gap, on purpose.** The old task never expired, so it relaunched Virtue
  after sign-in even when the user had turned off the "Virtue Tray" startup entry. Now, if the
  startup entry is off and Virtue doesn't run for more than a day (PC off, or nobody signed in),
  the task has expired and nothing starts Virtue at the next sign-in. We accept this rather than
  lengthen the lifetime or watch the startup entry's state: the time Virtue isn't running is
  reported to partners as a gap anyway.

- **Every minute** is Task Scheduler's floor for a repeating trigger (`Repetition/Interval` is
  `minInclusive="PT1M"`, and `schtasks.exe` rejects less). The core's late-wakeup threshold
  (CORE-002) is 2 minutes partly so a normal relaunch doesn't raise a tamper alert.
- **It launches the app execution alias** (`virtue-initiative.exe`, see `AppLaunchPath`) rather
  than `Environment.ProcessPath`. An MSIX app's process path contains its version, so after a
  Store update the old path no longer exists and every run fails with `0x80070002`.
  `explorer.exe shell:AppsFolder\...` is also version-independent but drops the command-line
  arguments the quiet relaunch needs. If the user switches the alias off (Settings > Apps > App
  execution aliases), the task falls back to the process path, which works until the next update
  and is repaired at the next launch.
- **Not `RegisterApplicationRestart`.** Windows' restart API registers fine for MSIX apps and
  Windows Error Reporting sees the crash, but nothing is relaunched.

To check a machine: `schtasks /Query /TN VirtueResidentWatchdog /XML`, and
`Get-ScheduledTaskInfo -TaskName VirtueResidentWatchdog` for `LastTaskResult`. The code is in
`Virtue.WindowsApp.Core/Interop/RestartWatchdog.cs` and `Virtue.WindowsApp/App.xaml.cs`.

## Store updates

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
thread. The window menu's **Check for Updates** item calls `RequestCheckNow()`, which releases
a semaphore that slice waits on — short-circuiting the cadence is its _only_ effect. Feedback
appears at the bottom of the menu via `SessionViewModel.UpdateCheckStatusText` ("Checking for updates..." →
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

### Coming back within seconds, not within a minute

The watchdog poll alone leaves the app gone for anywhere from 0 to 60 seconds after an update.
`UpdateRelaunchTask` (`Virtue.WindowsApp.Core/Interop/UpdateRelaunchTask.cs`, unit-tested) closes
that to a measured ~15s: immediately before each install attempt, `App.ScheduleUpdateRelaunch()`
registers a **one-shot** `VirtueUpdateRelaunch` task pointing at the `AppLaunchPath`
execution alias.

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
(`show-window-on-next-launch` in the app's folder, unit-tested) and flips the card to
"Installing the update. Virtue will close and restart itself within a minute." with the button
disabled. The flag is consumed by the next launch, which then shows the main window instead of
coming back quietly into the tray: the user asked for a restart, so the process vanishing with
nothing visibly returning is the wrong feedback. It is consumed unconditionally at startup, and
also by `ClearStagedUpdateAfterFailedInstall()` (where the process survived, so no launch is
coming), so a stale flag can't pop a window at some unrelated later launch. An automatic
window-closed update sets no flag and stays quiet.

The status line at the bottom of the window menu reports every terminal outcome — "No updates found.",
"Update found; downloading...", "Update downloaded.", "Update check failed.", "Update install did
not complete; it will be retried." — and the menu item short-circuits to "Update already downloaded."
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

### Testing an update without publishing one

Reaching any of the above normally requires publishing a newer package to a Store flight, so
there is a developer-only shortcut. Write a file named `debug-stage-update` in the app's folder
(see [Files on a running machine](README.md#files-on-a-running-machine)) — `DebugUpdateSentinel`
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
echo 2m > (Join-Path (Resolve-Path "$env:LOCALAPPDATA\Packages\VirtueInitiative.*\LocalState\Virtue") debug-stage-update)
```

### Reading the update log

`ui-startup.log` in the app's folder is the only observability channel on a Store install, so
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

## Integration test

`scripts/integration-test.ts` (the `client-windows-integration` CI job) checks that a real
device build gets screenshots to the API. `virtue_windows` has no daemon process or CLI to
drive, so a small `virtue-windows-ci-runner` binary makes the same calls the app makes
(initialize, start monitoring, sign in) against the library, then waits long enough for the
background thread to capture, hash and upload before exiting. It runs unpackaged with
`PROGRAMDATA` pointed at a temp directory, so it doesn't touch a real install. GitHub's Windows runners have an
interactive desktop, so GDI capture takes a real screenshot.
