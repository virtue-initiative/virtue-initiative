# Virtue Client Apps

This spec covers the platform apps (Linux, macOS, Windows, Android, iOS) that sit on top of the shared core. The core's own behavior (the daemon loop, screenshots, tamper detection, batching, upload, status fields) is specified in `client/core/SPEC.md` and cited here by `CORE-` IDs.

## CLIENT-001 Overview

Each platform app MUST run the shared core daemon (CORE-001) and supply only what the core cannot: screen capture, lock detection, system login/logout times and clocks (CLIENT-018), text recognition (CLIENT-019), a way for the user to sign in and out, a status display, and OS integration (autostart, updates, permissions).

All monitoring policy (capture cadence CORE-003, content detection CORE-004, uploads CORE-005, tamper detection CORE-002, status CORE-010) MUST come from the core. A platform MUST NOT reimplement it.

The API URL, capture interval and batch window are fixed at build time on every platform. No platform has a configuration file the user edits.

## CLIENT-002 Where monitoring runs and stores state

| Platform | Monitoring process                                                     | State directory                                            |
| -------- | ---------------------------------------------------------------------- | ---------------------------------------------------------- |
| Linux    | `virtue daemon`, a systemd user service, separate from CLI and tray    | `$XDG_STATE_HOME/virtue` (default `~/.local/state/virtue`) |
| macOS    | `virtue-daemon daemon`, a LaunchAgent, separate from the menu bar app  | `~/Library/Application Support/virtue/state`               |
| Windows  | inside the resident app process                                        | `%PROGRAMDATA%\Virtue\data`                                |
| Android  | inside the app process, run by the Accessibility service               | app-private `core-data`                                    |
| iOS      | inside the Safari extension, one tick per extension message (CORE-015) | App Group `group.org.virtueinitiative.virtueios`           |

- The core state file MUST be `event_state.json` in the state directory (CORE-016).
- Linux MUST also use `$XDG_CONFIG_HOME/virtue` (default `~/.config/virtue`) as its config directory. A build made with `VIRTUE_INSTANCE=<name>` MUST use `virtue-<name>` for both directory names, its service name and its tray id. An empty instance name MUST behave as no instance.
- Windows MUST remember the signed-in email in `%PROGRAMDATA%\Virtue\config\ui_state.json`, and macOS in `~/Library/Application Support/virtue/ui_state.json`. Android and iOS keep it in app preferences.

> OPEN: Windows keeps all state in machine-wide `%PROGRAMDATA%\Virtue`, so every Windows account on one PC shares one sign-in and one device — is per-user monitoring on shared PCs intended?

## CLIENT-003 Installing and first run

- Linux: the `.deb` MUST enable `virtue.service` globally for every user, start or restart it for every logged-in user, and enable the update timer only when the package was built with auto-update on (CLIENT-023). On Cinnamon it MUST add the panel applet that hosts tray icons if it is missing. Before `virtue login`, the daemon runs with capture disabled.
- macOS: the first launch MUST open the main window, request Screen Recording (CLIENT-025), register the app as a login item, and install and start the daemon LaunchAgent (CLIENT-024).
- Windows: the app is installed from the Microsoft Store as an MSIX. Launching it MUST start monitoring in-process, create the tray icon, register the watchdog (CLIENT-028) and show the main window.
- Android: the app MUST keep a signed-out user on the Accessibility setup guide until the Accessibility service is connected, and only then show the sign-in form (CLIENT-030). Every launch MUST ask for the notification permission (Android 13+) and for exemption from battery optimization when either is missing.
- iOS: after sign-in the user MUST be guided to turn on the Safari extension (CLIENT-032). Nothing is monitored until they do.

## CLIENT-004 Signing in

The sign-in form MUST take an email, a password and an optional device name, and MUST call CORE-008.

- Email and password MUST both be present before any request is made. Messages: macOS and iOS "Email is required" / "Password is required"; Windows "Email is required." / "Password is required."; Android "Enter your email and password."
- A blank device name MUST be replaced by the platform default: the hostname (Linux falls back to `linux-device`, macOS to `mac-device`), the Windows machine name, "Manufacturer Model" on Android (the model alone when it already starts with the manufacturer), or the iOS device name.
- On success the app MUST clear the password field, remember the account email locally (to prefill Report an Issue, CLIENT-016) and refresh its status.
- The form MUST offer a sign-up link to `https://app.virtueinitiative.org/signup` and, once signed in, a link to `https://app.virtueinitiative.org/partners` with the text: Add partners on the Virtue website. Open the Partners page and select "Invite partner".
- Sign-in needs a running daemon loop (CORE-007). Linux and macOS MUST fail with "failed to connect to daemon (is it running?)" when the daemon is unreachable; Windows with "monitoring is not running". Android MUST show the Accessibility guide instead of attempting sign-in while the service is disconnected, and MUST restart a paused loop before signing in.
- Linux `virtue login` MUST mask the typed password with `*`, abort on Ctrl+C or Ctrl+D, prompt `Device name [<hostname>]:` unless `--device-name` is given, and skip the email prompt when `--email` is given. On success it MUST print the capture probe result (CLIENT-020), the device id, the partners instruction and the monitoring summary, plus a warning when the probe failed.
- macOS MUST show "Login failed. Check your email and password and try again." when the error mentions unauthorized, bad request or login failed, and "Login failed: <error>" otherwise. Android MUST show "Sign in failed. <error>." (adding the period only when the error lacks terminal punctuation).

> OPEN: Android retries a sign-in that fails with "serialization error" after deleting the entire `core-data` directory, including any events still waiting to upload, although CORE-017 already recovers unreadable state — is the wipe still needed, and may it discard queued alerts?

## CLIENT-005 Signing out and monitoring while signed out

Sign out MUST ask for confirmation with: Signing out will deactivate this device and stop monitoring. Anyone monitoring you may be alerted. Logging in again will create a new device. (Linux prints it as a warning and defaults the prompt to No; `--yes` skips it. Windows shows "Anyone monitoring you may be alerted." on its own line.) Confirming MUST call CORE-009 and forget the remembered account email.

While signed out, no screenshots are taken (CORE-009):

- Linux, macOS and Windows: the daemon MUST keep running, so signing in again works without restarting anything. The macOS menu bar MUST NOT offer Sign Out; it is in the window only.
- Android: sign-out MUST also stop the daemon loop and remove the persistent notification.
- iOS: sign-out MUST also turn the monitoring switch off (CLIENT-032).

> OPEN: when native logout fails, Android deletes `core-data/auth.json`, a file that no longer exists (auth lives in `event_state.json`), so the fallback does nothing — should it be removed or retargeted?

## CLIENT-006 Status the user can see

Every app MUST show whether the user is signed in and a one-line monitoring state, and MUST give access to Status Details (CLIENT-007).

- Linux: `virtue status` (CLIENT-020) and the tray tooltip (CLIENT-021).
- macOS window headline, first match wins: "Signed out"; "Monitoring stopped" (daemon reported stopped, CLIENT-024); "Screen Recording off"; "Monitoring active"; otherwise "Starting…".
- Windows: see CLIENT-027.
- Android headline: "Signed out"; "Monitoring active" (loop running); "Monitoring paused" (user paused); otherwise "Waiting for first capture".
- iOS headline: "Signed out"; "Monitoring paused"; "Setup not finished"; "Safari permission needed"; "Monitoring active" (CLIENT-032).
- Every app MUST show its build label. Every GUI MUST show the "What Virtue Monitors" summary with a Learn more link to `https://virtueinitiative.org/help/what-virtue-monitors/<platform>`; Linux prints it after login and at the end of `virtue status`.

## CLIENT-007 Status Details

Status Details MUST render the CORE-010 fields, grouped as:

- Account: signed in, email, device name, partners (`<unknown>` when `partner_count` is absent).
- Queues: waiting for hash, waiting in batch, last batch upload.
- Capture: daemon state, last loop, last attempt, last screenshot, last skip reason.
- Recent errors: each with its time, context and message, or a none marker.
- Advanced: device id, API URL, hash base URL (`<default>` when unset), capture interval, batch window, and where the logs are (CLIENT-017).

Timestamps MUST be shown in local time with a relative age; absent values MUST show `<none>`. Skip reasons MUST read "Screen unchanged since the last upload", "Screen locked or screensaver active" or "Capture failed".

- macOS adds Screen Recording on/off, the build and a Reveal Logs button. Windows adds an "Open log folder" button. Android adds "Open log file", which shares the newest log file with a text viewer, and says "No log file has been written yet." or "No app on this device can open a text file." when it cannot.
- Linux `virtue status` MUST show the five newest errors and format ages as `Ns`/`Nm`/`Nh`/`Nd ago`, or "in the future" for a timestamp ahead of now.

## CLIENT-008 Test Screenshot

A signed-in user MUST be able to request a screenshot now. The app MUST call CORE-021, then poll status every 0.5 s for up to 20 s and report one outcome, with the same wording on every platform:

- Uploaded: "Screenshot uploaded. Check the web logs page to view it."
- Not taken, screen unchanged: "No screenshot was taken: the screen hasn't changed since the last one."
- Not taken, locked: "No screenshot was taken: the screen is locked or the screensaver is on."
- Capture failed: "The screen could not be captured. See the status page for details."
- Not taken, no reason: "No screenshot was taken."
- Still uploading at the deadline: "Screenshot captured, but it hasn't finished uploading yet. The client will keep trying in the background."

While waiting, the app MUST show "Screenshot uploading…" and MUST NOT start a second request. A failed request MUST show "Test screenshot failed: <error>". Transient messages clear after a few seconds (macOS 3 s, Android 2.5 s).

- Linux: `virtue test-screenshot`. macOS: window button and menu bar item, both disabled while Screen Recording is off. Windows: window button and a tray item shown only while signed in; the tray reports the outcome in a notification balloon. Android: status card button.
- iOS: "Capture Next Safari Page" only records the request; the extension captures the next Safari page it sees and uploads it with everything else waiting (CORE-021 via `tick_once`). The app does not wait for an outcome.

## CLIENT-009 Permissions

A missing capture permission MUST be explained in the app with steps to fix it. Capture attempts made while it is missing fail, and repeated failures reach partners as Capture Failed through the core.

- Linux: no OS permission. Capture needs a graphical session and a capture tool (CLIENT-018); `virtue login` probes it (CLIENT-020).
- macOS: Screen Recording (CLIENT-025).
- Windows: none.
- Android: the Accessibility service is the capture permission (CLIENT-030). Notifications and battery-optimization exemption are requested on each launch but are not required to capture.
- iOS: the Safari extension, its All Websites access and Private Browsing access (CLIENT-032).

## CLIENT-010 Starting automatically and staying running

Monitoring MUST start without user action after a reboot or sign-in to the OS, and SHOULD come back on its own after a crash.

- Linux: the user service starts with the graphical session and MUST restart 5 s after any exit (`Restart=always`). When the hardware clock is in local time it MUST wait up to 30 s for NTP sync before starting.
- macOS: the LaunchAgent MUST be `RunAtLoad` and `KeepAlive`. Every app launch MUST rewrite the agent if it changed, re-enable it and force-restart the daemon (`launchctl kickstart -k`). The app itself MUST be a login item.
- Windows: the package startup task (`VirtueTrayStartup`) MUST launch the app at sign-in, quietly into the tray; the watchdog relaunches it within a minute (CLIENT-028).
- Android: Android restarts the Accessibility service after boot, which starts the loop. On boot or package replacement, a signed-in app MUST start the persistent notification and a 15-minute keepalive job. The notification service MUST be sticky and MUST schedule its own restart 5 s after the app's task is removed.
- iOS: no background process. Monitoring runs only while the Safari extension is handling pages.

## CLIENT-011 Stopping or pausing monitoring

A signed-in user stopping monitoring MUST be asked to confirm, and a confirmed stop MUST be recorded with CORE-011 before monitoring stops, so partners see Monitoring Stopped by User and the gap is excused (CORE-002). The next start records Monitoring Resumed by User (CORE-022).

| Platform | Control                                                              | Source tag             | After stopping                                                 |
| -------- | -------------------------------------------------------------------- | ---------------------- | -------------------------------------------------------------- |
| Linux    | `virtue daemon stop [--yes]`                                         | `cli_daemon_stop`      | service stopped until `virtue daemon start` or next OS sign-in |
| macOS    | "Stop Monitoring & Quit" (window), "Stop Monitoring and Quit" (menu) | `mac_app_stop`         | agent unloaded and removed, app quits                          |
| Windows  | "Stop Monitoring" (window), Exit (tray)                              | `tray_stop_monitoring` | watchdog and relaunch task removed, app exits                  |
| Android  | "Pause Monitoring" / "Resume Monitoring"                             | `android_pause_button` | loop stopped, app stays open                                   |
| iOS      | "Pause Monitoring" / resume                                          | `ios_pause_button`     | monitoring switch off until resumed                            |

- Confirmation text: Linux "Warning: stopping the daemon will alert people monitoring you." with the prompt defaulting to No; macOS "Stop monitoring and quit?" / "This will stop monitoring on this device and quit Virtue. People monitoring you may be alerted."; Windows "Stop monitoring and close Virtue?" / "This will stop monitoring on this device and close the main window and tray app." / "People monitoring you may be alerted."; Android and iOS "This will stop monitoring on this device. People monitoring you may be alerted."
- Linux MUST say "<service> is already stopped." and do nothing when the service is inactive, and MUST NOT stop the service if the stop could not be recorded.
- While signed out, quitting MUST NOT ask for confirmation or record a user stop. macOS shows "Quit" in the menu and disables the window button; Windows Exit stops monitoring and exits.
- Android resume MUST ask "Resume monitoring on this device?" and, if Accessibility is off, MUST show the setup guide instead.

> OPEN: macOS stops the LaunchAgent even when the user stop could not be recorded (daemon unreachable), while Linux refuses to stop — should macOS fail closed like Linux?

> OPEN: how long a stop lasts differs: Linux, macOS and Windows resume at the next OS sign-in (service enablement, login item, startup task), Android at the next process start (the pause flag is in memory), and iOS only when the user resumes — which is intended?

## CLIENT-012 OS sign-out, shutdown and sleep

An OS sign-out or shutdown MUST stop the daemon cleanly without recording a user stop; the resulting gap is excused by login/logout evidence (CORE-002, CLIENT-018).

- Linux and macOS: the service manager's SIGTERM or SIGINT MUST make the daemon finish its current tick and exit (CORE-014).
- Windows: the app MUST stop monitoring when the tray host observes session logoff or system shutdown.
- macOS: the daemon MUST ask for an immediate batch upload after the system wakes from sleep.

## CLIENT-013 Updates

Shared rules for every platform that updates itself:

- A build made locally or for a pull request MUST NOT update itself: Linux only with `auto-update-enabled` = `1` (CLIENT-023), macOS only with a baked-in feed URL (CLIENT-026), Android only for stable release builds (CLIENT-031). Windows updates come from the Store listing or flight it was installed from (CLIENT-029). iOS updates come from TestFlight and the app does nothing itself.
- An update MUST follow the channel of the installed build: stable builds take stable releases, dev builds take dev releases (Linux, macOS). Android dev builds do not update at all.
- Installing an update MUST NOT require the user to stop monitoring first, and monitoring MUST NOT be stopped ahead of the install (macOS, Windows). The new version MUST come back on its own.
- A failed check or install MUST be retried later without user action.

## CLIENT-014 Uninstalling

- Linux: removing the package MUST remove the global enablement, disable the update timer, and stop and disable the service for every logged-in user. Upgrades run the same stop, then CLIENT-003's start.
- macOS, Windows, Android and iOS: uninstalling is left to the OS; the apps do nothing at uninstall.

> OPEN: no platform records anything at uninstall, so partners only notice through missing activity and check-ins — should uninstall send Monitoring Stopped by User where possible?

> OPEN: macOS leaves the KeepAlive LaunchAgent plist pointing at the deleted app, and Windows leaves the `VirtueResidentWatchdog` task running every minute — should these be cleaned up?

## CLIENT-015 Communication between UI and daemon

- Linux and macOS: the CLI, tray and app MUST reach the daemon over the Unix socket `daemon.sock` in the state directory. The daemon serves one connection at a time, so callers SHOULD open a fresh connection per request rather than holding it.
- When the socket is unreachable, status readers MUST fall back to computing status from `event_state.json` with `is_running` false (CORE-010), so a stopped daemon still reads as signed in.
- The macOS app MUST distinguish a refused connection (Stopped) from a connection that does not answer (Unreachable, shown as "Unreachable (busy)") and MUST NOT treat Unreachable as stopped.
- Windows: native calls MUST return nothing on success or an error message, and status calls MUST return JSON. A status payload that is not JSON MUST surface as an error. Session status MUST carry `loggedIn`, `buildLabel` and `email`; monitor status MUST carry `state`, `loggedIn`, `pendingRequestCount`, `lastError`, every CORE-010 field in camelCase, and `logDirectory`. Validation errors MUST read "email is required", "password is required" and "message is required"; a call that needs the monitor while it is not running MUST fail with "monitoring is not running". Strings returned by the native layer MUST be released through it.
- iOS: the app and the Safari extension MUST share the monitoring switch, the Capture Next Safari Page request and the extension's heartbeat and capture state through App Group defaults.

## CLIENT-016 Report an Issue

Every app MUST let the user send a report to the Virtue Initiative team (API-042), signed in or not.

- A message is required. Blank input MUST be refused before sending: "Please describe the issue." in GUIs, "No message entered; nothing was sent." on Linux.
- A contact email is optional. GUIs prefill it from the remembered account email; Linux asks for it only when not signed in.
- When signed in, the report MUST be attributed to this device using credentials read from disk, so it works while the daemon is down.
- Platform details MUST be included and MUST never be empty: Linux "Linux <kernel>; <NAME> <VERSION>" from `/etc/os-release` (quotes stripped; a missing NAME drops that part; `unknown` when nothing is known); macOS "<product> <version> (Build <build>)" or "macOS (unknown version)"; Windows "<product>; <version> (Build <build>)" or "Windows (unknown version)".
- Logs: GUIs offer a checkbox, on by default, to attach recent logs (CLIENT-017). Linux always attaches the last day of the service journal when there is one, and says how many KB it is. Logs MUST be redacted of known secrets and trimmed to the size limit, keeping the newest bytes.
- Linux MUST list exactly what will be sent and ask "Send this report?" (default Yes); `--yes` skips it, `--message` and `--contact-email` skip the prompts.
- Success MUST show "Thanks — your report was sent to the Virtue Initiative team." Failure MUST show the error; GUIs keep the form open.

> OPEN: the log checkbox says "last two days" on macOS and iOS and "last day" on Windows and Android, but all four attach the same files (those modified in the last 24 hours) — which wording is right?

## CLIENT-017 Diagnostic logs

- Linux: logs MUST go to the journal, readable with `journalctl --user -u virtue.service`.
- macOS, Windows, Android and iOS: logs MUST be written to daily files `virtue.<date>.log`, keeping the 14 newest: `~/Library/Logs` (plus launchd fallback files `virtue-daemon-launchd.log` and `virtue-daemon-launchd.error.log`), `%PROGRAMDATA%\Virtue\data\logs`, `core-data/logs` and the App Group `logs` directory.
- Setting up logging MUST be safe to repeat in one process: a second call MUST NOT fail or panic.
- Windows MUST also append app lifecycle and update events to `%PROGRAMDATA%\Virtue\ui-startup.log`, including a line for a check that found no update.
- The log level MUST follow the build type; Linux and macOS also honor `RUST_LOG`.

## CLIENT-018 Platform signals

Each platform MUST supply the following to the core. A signal that cannot be read MUST fail safe: lock state reads as unlocked (falling back to the change check), and login/logout times read as unavailable.

| Platform | Capture                                                    | Locked or screensaver                                                  | Last login                           | Last logout                                                                 |
| -------- | ---------------------------------------------------------- | ---------------------------------------------------------------------- | ------------------------------------ | --------------------------------------------------------------------------- |
| Linux    | all screens: `grim` on Wayland, else `import`, then `maim` | logind `LockedHint`, else freedesktop or GNOME screensaver `GetActive` | logind session start                 | last journal entry of the previous boot                                     |
| macOS    | main display via `screencapture`                           | session screen-locked flag, or a screensaver process running           | newest real login in `last`          | newest of a `shutdown time` entry or a finished session's end in `last`     |
| Windows  | every monitor as one image                                 | screensaver running, or the input desktop cannot be opened             | current logon session start          | newest of the registry `ShutdownTime` and the last System event before boot |
| Android  | default display via Accessibility (Android 11+)            | screen not interactive                                                 | device boot time                     | unavailable                                                                 |
| iOS      | visible Safari tab                                         | (not used)                                                             | tamper detection disabled (CORE-002) | tamper detection disabled                                                   |

- Linux MUST prefer Wayland when both Wayland and X11 are available, and MUST treat "no graphical session detected", "x11 display unavailable", "unable to open x server" and "can't open display" (any case) as the session being absent rather than a capture fault.
- Linux previous-boot parsing MUST accept the timestamp as a number or a string of microseconds, and MUST return unavailable for no previous boot, an empty list or invalid JSON.
- macOS `last` parsing MUST skip `reboot` and `shutdown` pseudo-entries when finding a login, MUST ignore sessions still logged in, and MUST compute a session's end from its start plus its duration.
- Windows event-log parsing MUST read the single-quoted `SystemTime` attribute and return unavailable for an empty result or a malformed timestamp.
- Wall-clock time MUST be UTC milliseconds since the epoch. The suspend-excluding clock (CORE-002) MUST be `CLOCK_MONOTONIC` on Linux, `mach_absolute_time` on macOS, unbiased interrupt time on Windows and `uptimeMillis` on Android. Both MUST be positive, and the suspend-excluding clock MUST advance.

> OPEN: macOS captures only the main display, while Linux and Windows capture every screen — should macOS capture all displays?

## CLIENT-019 Text recognition for redaction

Each platform MUST supply an OCR engine for CORE-004 that returns every detected word with its pixel bounding box (origin top-left) plus the recognized text.

- Linux: Tesseract with English by default, using the language data bundled in the package and the system data otherwise, with automatic page layout. Words below confidence 50 MUST be dropped from the text but kept as boxes; a word with no confidence counts as 100. Lines with fewer than 3 characters MUST be dropped from the text. Paragraphs MUST be joined by a blank line and lines by a newline. HTML entities MUST be decoded.
- macOS and iOS: Apple Vision. Windows: Windows OCR.
- Android: ML Kit, called back from the core. Each record MUST be `text|left|top|right|bottom`; empty records, records without five fields and empty text MUST be skipped. Recognition MUST fail with an initialization error if no engine was registered.

## CLIENT-020 Linux: `virtue` command

| Command                                                                      | Behavior                                                                                             |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `virtue login [--email E] [--device-name N]`                                 | CLIENT-004                                                                                           |
| `virtue logout [--yes]`                                                      | CLIENT-005; prints "Logged out. Monitoring is disabled on this device until you run `virtue login`." |
| `virtue status [--json]`                                                     | CLIENT-007                                                                                           |
| `virtue daemon`                                                              | runs the daemon and tray (used by systemd)                                                           |
| `virtue daemon start`                                                        | `systemctl --user start` the service                                                                 |
| `virtue daemon stop [--yes]`                                                 | CLIENT-011                                                                                           |
| `virtue test-screenshot`                                                     | CLIENT-008                                                                                           |
| `virtue report-issue [--message M] [--contact-email E] [--yes]`              | CLIENT-016                                                                                           |
| `virtue dev upload-log` / `dev add-log [--risk R] [--title T] [--details D]` | queue a Developer log, sent now or with the next batch                                               |
| `virtue dev send --type T` (debug builds)                                    | queue any log type, or `all`                                                                         |

- `--risk` MUST be a number from 0 to 1 and defaults to 0.5. `virtue --version` MUST print the build label.
- The login capture probe MUST report one of: Wayland capture working; Wayland capture failing, with advice to use X11 or a compositor that allows `grim`; X11 working; X11 failing, with advice to install ImageMagick or `maim`; or no graphical session, with advice to run `virtue login` inside the desktop session.
- `status --json` MUST output the CORE-010 fields plus `backend`, `service_name`, `service_active`, `state_dir`, `log_command`, and the older keys `logged_in`, `running` and `base_api_url`.

> OPEN: `virtue dev upload-log` always sends risk 1.0 but prints the `--risk` value it was given — should it honor `--risk` or stop accepting it?

## CLIENT-021 Linux: tray icon

- The daemon MUST show a tray icon titled "Virtue" when a StatusNotifier host is available, and MUST keep monitoring without it otherwise.
- The tooltip MUST read "<email> — nothing queued." or "<email> — <n> waiting to upload." followed by "Run 'virtue status' from a terminal for details." when signed in, and "Not signed in. Run 'virtue login' from a terminal." otherwise. It MUST refresh every 15 s.
- With no session bus it MUST check again every 5 s. If no tray host exists it MUST retry after 5 s, 10 s and 60 s, then give up until the daemon restarts. Other tray failures MUST retry every 30 s. Repeated identical failures SHOULD be logged at most every 10 minutes.

## CLIENT-022 Linux: service

- The user unit is `virtue.service` (`virtue-<instance>.service` for instance builds), running `/usr/bin/virtue daemon` after the graphical session and network are up. When neither `DISPLAY` nor `WAYLAND_DISPLAY` is set it MUST pick the first X11 socket, and it MUST use `~/.Xauthority` when `XAUTHORITY` is unset.
- The officially supported install is the `.deb` on Debian/Ubuntu-family distributions with systemd. It bundles Tesseract and its English data.

## CLIENT-023 Linux: updates

- A system timer MUST run `/usr/lib/virtue/update-check.sh` as root 10 minutes after boot and then every 6 hours, with up to 30 minutes of random delay, catching up on missed runs.
- The installed channel MUST come from `/usr/lib/virtue/release-tag`: a tag ending in `-dev` follows the newest prerelease, any other tag the newest full release. Releases without a `.deb` for this architecture MUST be skipped.
- An update MUST be installed when the chosen release's build label differs from `/usr/lib/virtue/build-label`. The download MUST be a valid `.deb`; a dependency failure MUST fall back to `apt-get install -f -y`. Installing re-runs CLIENT-003's restart.
- Only one check MUST run at a time. A failed check exits non-zero and is retried at the next timer run.

## CLIENT-024 macOS: app, menu bar and daemon

- The app is a menu bar app that opens its window at launch. The menu MUST offer Open Virtue; Log In when signed out; Test Screenshot when signed in; Check for Updates when updates are enabled; and Quit or Stop Monitoring and Quit.
- On launch the app MUST terminate any other running copy of itself before doing anything else.
- The app MUST poll the daemon every 2 s. It MUST report "Virtue background service stopped unexpectedly. Relaunch the Virtue app to continue monitoring." only after the daemon has read as stopped for 20 s, and MUST NOT report it within 30 s of launch or after the user chose to quit. A failure to start the agent MUST be shown as "Failed to start background service: <error>".
- The daemon executable is `Contents/MacOS/virtue-daemon` inside the app, run by the agent `org.virtueinitiative.virtue.daemon` (`~/Library/LaunchAgents/org.virtueinitiative.virtue.daemon.plist`). `virtue-daemon status` MUST print `logged_in`, `running`, `pending_request_count`, `device_id`, `capture_interval_seconds`, `batch_window_seconds`, `base_api_url` and `backend: screencapture`, one per line.

## CLIENT-025 macOS: Screen Recording permission

- The app MUST request Screen Recording at every launch where it is missing, before sign-in. macOS shows its own prompt only once per app.
- While it is missing, the window MUST show an "Action Needed" card ("Allow Screen Recording") with Open System Settings (deep link to the Screen Recording pane) and Restart Virtue, and Test Screenshot MUST be disabled with the hint "Allow Screen Recording to use this feature".
- macOS only reveals a grant to a new process, so the app MUST NOT re-check the permission while running; the user restarts through macOS's "Quit & Reopen" or Restart Virtue, and the relaunched app restarts the daemon.
- The bundle id `org.virtueinitiative.virtue.mac`, the daemon name `virtue-daemon` and the signing team MUST stay fixed so updates keep the grant.

## CLIENT-026 macOS: updates and replaced app

- With a feed URL baked in, the app MUST check `https://virtueinitiative.org/appcast.xml` at launch and every 6 hours, and MUST download, install and relaunch without asking. Check for Updates MUST run the same silent path. Dev builds MUST accept only `dev`-channel items; stable builds only untagged items.
- The daemon MUST keep running through the install; the relaunched app restarts it on the new binary (CLIENT-010).
- If the app bundle is replaced on disk outside an update, the app MUST relaunch into the new version once two consecutive polls see the same new build label, and MUST NOT do so while an update is in progress.

> OPEN: update results ("Up to date.", "Update check failed: …") are computed but never displayed, so Check for Updates gives no feedback — should the menu or window show them?

## CLIENT-027 Windows: resident app, tray and window

- Only one instance MUST run. A second launch MUST hand off to the running instance, which shows its window unless the launch was quiet. A launch is quiet when it comes from the startup task or carries `--restarted-by-watchdog`, unless the update window flag is set (CLIENT-029).
- Closing the window MUST hide it to the tray. The tray menu MUST offer Open, Test Screenshot (signed in only), Report an Issue and Exit, and MUST report session logoff and system shutdown (CLIENT-012). Where no tray icon can be hosted, the tray MUST do nothing and have no window handle.
- The window MUST first open at 760×640 effective pixels scaled by the window's DPI (a non-positive DPI counts as 96), clamped to 90% of the monitor's work area and at least 1 pixel, and centered in that work area.
- Status text: signed out "Sign in to start monitoring."; running "Monitoring is active on this device."; starting "Monitoring is starting for this device."; error "Monitoring needs attention: <error>" (or without the error); stopped "Monitoring is stopped on this device."; anything else "Monitoring state is updating."
- Tray tooltip: "Virtue: loading status", "Virtue: monitoring active", "Virtue: starting monitoring", "Virtue: <error>" or "Virtue: monitoring error", "Virtue: sign in required", otherwise "Virtue: monitoring stopped", with " (update ready)" appended while an update is staged.
- Before the first status load the app MUST show "Loading..." for signed in and account, and the state `loading`. Once loaded, the signed-in line MUST read "Yes" or "No" and the account line MUST show the email, or "Not signed in" when there is none. A signed-in session whose monitor reports signed out MUST show `starting`. Signed out MUST clear the device id, error, queue count and last screenshot. The device id and email MUST fall back to the daemon's values, the email only while signed in. The email field MUST follow the account while signed in and keep what the user typed while signed out. Displayed states MUST show underscores as spaces. The build line is "Build <label> | Windows package <version>", or "Build <label>" without a package version ("Build unknown" before load).
- The app MUST refresh monitor state every 5 s without changing the signed-in state, and fully after each action. While an action runs, others MUST be ignored and a progress message shown ("Signing in...", "Signing out...", "Screenshot uploading...", "Sending report..."); a failure MUST be shown as the error text and status text.

## CLIENT-028 Windows: watchdog

- At every launch the app MUST register the per-user task `VirtueResidentWatchdog`, running every minute with least privilege, that starts the app with `--restarted-by-watchdog`. A launch while the app runs is a no-op through the single-instance hand-off.
- The task MUST launch the execution alias `%LOCALAPPDATA%\Microsoft\WindowsApps\virtue-initiative.exe` when it exists, falling back to the running executable, and MUST be skipped when neither is known.
- Exit MUST delete the task first. Updates MUST NOT delete it.

## CLIENT-029 Windows: Store updates

- The app MUST check the Store 4 hours after a successful check; after failures it MUST retry after 5 minutes, doubling up to 1 hour. A non-positive failure count counts as none. Check for updates MUST trigger a check immediately, or report "Update already downloaded." when one is staged.
- The check line MUST report "Checking for updates...", "No updates found.", "Update found; downloading...", "Update downloaded.", "Update check failed." or "Update install did not complete; it will be retried.", and MUST NOT go blank afterwards.
- Downloads and installs MUST use the silent Store calls when allowed. An install that needs consent MUST show the window and retry interactively.
- Once an update is staged: with the window hidden and no action running (CLIENT-027), the app MUST install at once. With the window visible it MUST show "Virtue will restart to update in <countdown>" with "Restart now to update", and MUST hide the window to force the install 6 hours after staging, never while an action is running. Countdown: "<h>h <m>m" from one hour up, "<m>m" (at least 1) below, "any moment" at or past the deadline. Only one install MUST proceed at a time.
- The app MUST exit only after a successful install. Any other outcome MUST clear the staged update, notice, countdown, tooltip suffix and installing state, and leave the retry to the check loop.
- Before each install attempt the app MUST schedule the one-shot task `VirtueUpdateRelaunch` 15 s ahead: a time trigger with seconds precision and a 1-minute window, no user id, deleted 30 s after it expires, launching the CLIENT-028 path with `--restarted-by-watchdog`, with paths XML-escaped. It MUST be cancelled at launch, on Exit and after a failed install.
- "Restart now to update" MUST set `%PROGRAMDATA%\Virtue\show-window-on-next-launch` (creating its folder) and show "Installing the update. Virtue will close and restart itself within a minute." The next launch MUST consume the flag exactly once and open the window. A failed install MUST also consume it.
- Developer shortcut: a file at `%PROGRAMDATA%\Virtue\debug-stage-update` MUST be consumed (read, then deleted) within about 5 s and stage a simulated update that installs without the Store. Its content MAY set the deferral cap as `<n>s`, `<n>m`, `<n>h` or a bare number of minutes, case-insensitive and trimmed. Empty, non-positive or unknown content stages with the normal cap.

## CLIENT-030 Android: Accessibility and background services

- The Accessibility service "Virtue Screen Monitor" is the capture permission and MUST run the daemon loop while connected and not paused. Turning it off MUST stop the loop without recording a user stop.
- The setup guide MUST match the running Android version. On Android 13+ it MUST add the "Allow restricted settings" steps for apps installed from a downloaded file, and on Android 13 and 14 also when the first launch was such an install (an in-app update does not lift the block there).
- The persistent notification "Monitoring active" (channel "Virtue monitoring") MUST be shown while signed in. If Android refuses to start it, monitoring MUST continue without it.
- Restarting the loop MUST queue the resume (CORE-022) rather than wait for it, and MUST wait up to 15 s for a previous loop to finish stopping.

## CLIENT-031 Android: updates

- Stable release builds MUST check `https://virtueinitiative.org/android-update.json` every 6 hours and when the app opens, on unmetered networks only.
- The feed MUST be `{"version", "url", "sha256"}`. It offers no update when it is not JSON, is `{}`, or when the version is not dot-separated non-negative integers, the URL is not `https://` (`http://` allowed only in debug builds), or the hash is not 64 hex digits (case-insensitive).
- An update MUST be offered only when the feed version is numerically newer, comparing part by part with missing parts as 0. Equal or unparseable versions are not newer.
- The APK MUST match the feed's SHA-256, have this app's package name and a higher version code, or be discarded. A downloaded APK MUST be rechecked against its hash before installing.
- On Android 12+, when Virtue is its own installer of record and may install packages, it MUST install silently, but only while the app is not on screen. Otherwise it MUST post "Virtue update available" / "Tap to install version <v>." and show "Install Update <v>", which opens Android's confirmation. A successful install MUST clear the downloaded APK and notification.

## CLIENT-032 iOS: Safari extension

- Monitoring covers Safari only: the extension "Virtue Safari Capture" captures the visible tab, and the daemon in the extension takes screenshots from those frames (CORE-015). Tamper detection is disabled (CORE-002, CORE-019).
- Headline state: signed out; paused (monitoring switch off); setup not finished (the extension is not known to be on, or has never delivered a frame); Safari permission needed (All Websites or Private Browsing known to be denied); otherwise active. It MUST NOT depend on whether Safari is open.
- Check Extension MUST open a check page in Safari, which reports back with `virtueinitiative://check-extension?extension=on|off&all_sites=0|1&private=0|1`. Missing flags keep their last values. Any extension message after the check started also counts as on.
- Status Details MUST show the extension's last check-in, which counts as stale after 10 s, its last frame, last page, last error and daemon state.
- Pause MUST record CORE-011 and turn the switch off; capture stops at the extension's next message. Resume MUST record CORE-022 and turn it on. Signing in MUST turn it on.
