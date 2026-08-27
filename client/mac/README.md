# Virtue macOS Client

This client has two modes in one binary:

- Tray app (default): menu bar icon + login/logout dialogs
- Daemon (`daemon`): background capture + upload worker

## Behavior

- On tray app launch, it installs/starts a LaunchAgent (`org.virtueinitiative.virtue.daemon`) so the daemon starts immediately and is registered for future login/reboot launches.
- Login/logout flows use the shared device API (`/d/...`) and shared core auth command behavior.
- Daemon idles until login is complete (device token + device ID + E2EE key).
- Menu action `Open Virtue`:
  - If logged out: prompts for email/password.
  - If logged in: shows signed-in state with `Stop Monitoring` / `Logout`.
- `Stop Monitoring` asks for confirmation, records an explicit user stop intent, unregisters the daemon LaunchAgent, and exits the tray app.
- `Logout` asks for confirmation, alerts observers through the shared core logout flow, clears local login state, and unregisters the daemon LaunchAgent.
- Only one instance runs: on launch the app terminates any other running instance of its own
  bundle id, so dragging a new `Virtue.app` over a running one can't leave two menu bar icons
  from two different versions (issue #539).
- If the app bundle is replaced on disk while the app is running (a manual drag-install rather
  than an auto-update), the app notices within a couple of status polls and relaunches itself
  into the new version, which in turn restarts the daemon on the new binary. No
  "Quit monitoring and exit" / manual restart needed.
- Closing behavior:
  - `Stop Monitoring` is a full stop: tray icon exits, background daemon is stopped, and startup relaunch is disabled until the app is opened again.
  - `Logout` also disables startup relaunch until the app is opened again.
  - Opening the Virtue app again recreates/re-enables the LaunchAgent and starts background monitoring.

## Lifecycle logs

macOS lifecycle behavior follows the shared core lifecycle model, analogous to Linux:

- Service pings are recorded every 60 seconds so forced stops can be detected on the next start.
- Startup is detected from `kern.boottime`.
- Shutdown is best-effort via `NSWorkspaceWillPowerOffNotification` plus the daemon stop signal.
- If launchd delivers the stop signal before the power-off notification, the next boot upgrades a recent `unknown` stop marker into `system_shutdown` so reboot cycles stay zero-risk.
- Suspend and wake are tracked from `NSWorkspaceWillSleepNotification` / `NSWorkspaceDidWakeNotification`.
- Explicit tray-initiated stop prompts for confirmation before recording a user-requested stop.
- Logout prompts for confirmation before sending the shared core logout alert.

The tray app waits for the daemon to come up, but it now stays open and keeps polling if startup confirmation is delayed instead of immediately failing closed after a short timeout.

`system_shutdown` is best-effort and may still be missed on abrupt power loss or forced termination.

## Auto-update

The app updates itself with [Sparkle](https://sparkle-project.org) (2.9.6, pinned in
`mac/project.yml`). Checks run 6-hourly and on launch; an available update is downloaded,
installed, and relaunched **without prompting** — `VirtueUpdateDriver` in
`app/Sources/UpdateController.swift` is a headless `SPUUserDriver` that answers every Sparkle
question with "install now". This is deliberate on a monitoring client: a dismissible
"update later" is a bypass, and Sparkle's default "install on quit" would never fire in a
menu bar app that is a login item and effectively never quits. `Check for Updates` in the
menu runs the same silent path on demand.

The daemon is intentionally **not** stopped for the install. It runs from a binary inside the
bundle Sparkle replaces, but the running process keeps its old inode, and the relaunched app's
`ensureDaemonRunning` (`launchctl kickstart -k`) swaps it onto the new binary seconds later.
Stopping it first would fail unsafe — if the relaunch never happened, monitoring would be
silently off. Keep that window short: `lifecycle::tick` (CORE-002) raises a tamper alert on a
single late wakeup over 2 minutes.

### Enablement

Auto-update is off unless the app was built with `VIRTUE_ENABLE_AUTO_UPDATE=1`, which bakes
`SUFeedURL`/`SUPublicEDKey` into `Info.plist`; an empty feed URL means `UpdateController`
never starts the updater at all. This mirrors the Linux package's
`/usr/lib/virtue/auto-update-enabled` flag, and means a locally built app can never replace
itself with whatever is currently on GitHub Releases. Only the release-branch CI job sets it,
and only when a real Developer ID signing identity was available — Sparkle refuses an update
not signed by the same team, so an ad-hoc build would ship an updater that could never
install anything.

### Versions and channels

Sparkle orders updates by `CFBundleVersion`, which for this app is `<VERSION>.<commit-minutes>`
(e.g. `0.1.0.29796620`) — see `virtue_mac_bundle_version` in `client/scripts/version.sh`. The
fourth component is the commit timestamp in minutes, computed at build time, so dev-channel
builds between version bumps still compare correctly and **nothing has to be regenerated or
committed** for an update to be offered. It is deliberately not `APPLE_BUILD_NUMBER`: that
value is shared with iOS, where App Store submission caps `CFBundleVersion` at three integer
components.

One feed at `https://virtueinitiative.org/appcast.xml` serves both channels. Stable items are
untagged; dev items carry `<sparkle:channel>dev</sparkle:channel>`, which only dev builds opt
into via `SPUUpdaterDelegate.allowedChannels`. The feed is assembled by
`landing/scripts/build-appcast.mjs` from per-release `appcast-item-macos.xml` fragments that
`mac/scripts/make-appcast-item.sh` signs on the macOS runner. GitHub Releases can't host the
feed itself: `releases/latest/download/` skips prereleases, and a per-tag URL never contains
anything newer than the build it shipped with.

### Screen Recording permission across updates

TCC keys Screen Recording consent on the executable path plus its designated requirement
(signing identifier + team), not its cdhash — so replacing the app in place keeps permission
and users are **not** re-prompted on every upgrade. Three things are load-bearing for that and
must not change without accepting a one-time re-prompt for every existing user:

- the bundle identifier `org.virtueinitiative.virtue.mac`
- the daemon binary name `virtue-daemon` (its signing identifier)
- the Team ID `Y2Z8ZS4D33`

Moving between an ad-hoc-signed local build and a Developer ID release build does re-prompt;
that is unavoidable and only affects developers.

### Signing note

`build-app.sh` signs Sparkle's nested helpers (`Autoupdate`, `Updater.app`, and the two XPC
services) explicitly, inside out, before signing the framework and the app. `xcodebuild`
signs the outer framework when embedding it but leaves those helpers carrying the ad-hoc
signature from the SPM binary artifact, which passes `codesign --verify --deep --strict`
locally and then fails notarization. The build asserts the team identifier on each nested
binary so this can't regress silently. The outer re-sign is deliberately **not** `--deep`,
which Sparkle documents as breaking its updater and Apple deprecates.

## Local configuration

There are no runtime config overrides — `api_base_url`, `capture_interval_seconds`, and
`batch_window_seconds` are compile-time constants baked into the binary via `env!()`
(see `client/core/build.rs`). To set local values for development, copy
`.env.example` (repo root) to `.env` (gitignored) and edit it; real process/CI env
vars always take precedence over that file, and `~/.config/virtue-dev.env` (see root
`AGENTS.md`) fills in anything `.env` doesn't set. Rebuild for changes to take effect.

The mac client stores shared core state under:

`~/Library/Application Support/virtue/state`

## Screen capture permission

macOS may block screenshot capture until Screen Recording permission is granted for the app/binary.
If captures fail, grant permission under:

`System Settings -> Privacy & Security -> Screen Recording`

## Build

From `client/`:

```bash
cargo build --release -p virtue-mac
```

## Build `.app`

```bash
./mac/scripts/build-app.sh
```

Creates:

`client/target/macos/Virtue.app`

## Build `.dmg` (drag to Applications)

```bash
./mac/scripts/build-dmg.sh
```

Creates:

`client/target/macos/virtue-macos-<version>.dmg`
