# macOS client design

Why the macOS-specific parts work the way they do. The behavior is specified in
`client/SPEC.md` (CLIENT-024 to CLIENT-026). For building and running, see [README.md](README.md).

## Process model

The app is two processes from one bundle:

- **The menu bar app** (`app/Sources/`, SwiftUI) shows the window and menu. It calls Rust through
  `rust/src/lib.rs`, which talks to the daemon over the core's IPC socket (CORE-025).
- **The daemon** (`Contents/MacOS/virtue-daemon`, built from `src/`) runs the core's loop. A
  LaunchAgent, `org.virtueinitiative.virtue.daemon`, keeps it running across sign-ins and
  reboots. The app installs and starts the agent at every launch (`src/launch_agent.rs`).

Stop Monitoring and Sign Out both unregister the agent, so monitoring stays off until the user
opens Virtue again, which re-registers it.

The daemon also listens for wake-from-sleep (`src/power.rs`, `IORegisterForSystemPower`) and
flushes the batch right after waking. That is only so uploads don't wait for the next tick. It
plays no part in tamper detection (CORE-002).

Only one app instance runs: at launch it terminates any other running copy of its own bundle
id, so dragging a new `Virtue.app` over a running one can't leave two menu bar icons. If the
bundle is replaced on disk outside an update, the app notices within a couple of status polls
and relaunches into the new version, which restarts the daemon on the new binary.

## Screen Recording permission

The app requests Screen Recording at every launch where it is missing, before sign-in, so the
first screenshot after sign-in never fails for lack of it (issue #632). macOS shows its prompt only
once per app, ever, so after that the window's "Allow Screen Recording" steps send the user to
`System Settings -> Privacy & Security -> Screen & System Audio Recording` (`Screen Recording`
before macOS 15) with a deep link.

macOS caches the permission answer for the life of a process, so a running process can't see the
permission change. The app and the daemon both ask a new `virtue-daemon check-screen-capture`
process instead: the app at every status poll while its window is open, and the daemon before each
capture. The window leaves "Allow Screen Recording" within a few seconds of the user turning Virtue
on, with no restart, whether they click "Quit & Reopen" or "Later" (CLIENT-025).

The user-facing walkthrough, with screenshots, is `landing/src/content/download-instructions/mac.md`.

### Keeping the permission across updates

TCC keys Screen Recording consent on the executable path plus its designated requirement
(signing identifier + team), not its cdhash — so replacing the app in place keeps permission
and users are **not** re-prompted on every upgrade. Three things are load-bearing for that and
must not change without accepting a one-time re-prompt for every existing user:

- the bundle identifier `org.virtueinitiative.virtue.mac`
- the daemon binary name `virtue-daemon` (its signing identifier)
- the Team ID `Y2Z8ZS4D33`

Moving between an ad-hoc-signed local build and a Developer ID release build does re-prompt;
that is unavoidable and only affects developers.

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

## Integration test

`scripts/integration-test.ts` (the `client-macos-integration` CI job) runs the real
`virtue-mac` daemon binary directly, with no launchd and no `.app`, then signs it in over its
IPC socket with a small `virtue-mac-ci-login` helper. That is the same call the app's Rust
bridge makes. It runs with `$HOME` pointed at a temp directory, so it doesn't touch a real
install.

CI runners have no Screen Recording grant, and there is no headless way to give one. So the
test builds the daemon with the `mock-capture` Cargo feature (`src/capture.rs`), which returns a
fixed PNG instead of calling `screencapture`. The rest of the pipeline (classify, upload, hash,
batch) runs for real. Only `--features mock-capture` turns it on, and `build-app.sh` and
`build-dmg.sh` never pass it, so it can't reach a shipped build.
