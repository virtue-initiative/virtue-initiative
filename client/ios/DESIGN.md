# iOS client design

Why the iOS app works the way it does. The behavior is specified in `client/SPEC.md`
(CLIENT-032) and `client/core/SPEC.md` (CORE-015). For building and running, see
[README.md](README.md).

## Safari only

iOS gives third-party apps no way to capture other apps' screens unattended. The app used to
use a ReplayKit broadcast, which the user has to start from Control Center and which ends when
they or iOS stop it, so it was removed. A Safari Web Extension can capture the visible tab
whenever a page is open, without the user doing anything after setup. That covers Safari and
nothing else, which is the trade-off the app makes.

The extension's bundle id still ends in `.broadcast` (`project.yml`), left over from the
broadcast extension. Don't rename it: users' Safari settings for the extension are keyed on it.

## The core runs in the extension

There is no long-running background process on iOS, so the core's daemon lives in the Safari
extension's process:

1. `content.js` asks for a capture from the page; `background.js` captures the visible tab and
   sends the image to the native handler.
2. `SafariWebExtensionHandler.swift` keeps the latest frame in memory and runs one daemon tick
   per extension message (CORE-015). The daemon takes its screenshot from that frame through
   the same capture callback the other platforms use.
3. State and the extension's last check-in live in the App Group
   `group.org.virtueinitiative.virtueios`, so the app can show status without running the
   daemon itself.

The Rust bridge (`rust/src/lib.rs`) holds one `Daemon` per process, the same shape as Android's
JNI bridge.

## No tamper detection

The extension's process is suspended the moment the device locks, gets no notice of it, and has
no API for boot, shutdown or sign-in times. Every pause looks like every other, so a late wakeup
means nothing there. `IosPlatformHooks::lifecycle_enabled()` returns `false`, and the daemon
skips the late-wakeup check (CORE-002). Capture and upload run as on every other platform.
