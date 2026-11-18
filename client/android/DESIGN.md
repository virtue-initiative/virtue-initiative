# Android client design

Why the Android-specific parts work the way they do. The behavior is specified in
`client/SPEC.md` (CLIENT-030 and CLIENT-031). For building and running, see
[README.md](README.md).

## What runs where

Everything runs in the app's process:

- **`VirtueAccessibilityService`** is both the capture permission and the host of the daemon
  loop. It takes screenshots with `AccessibilityService.takeScreenshot()` and runs
  `nativeRunDaemonLoop` while it is connected and monitoring isn't paused. Turning
  Accessibility off stops the loop.
- **`ScreenshotService`** is a small foreground service that only holds the "Monitoring active"
  notification and keeps the process alive. If Android refuses to start it, monitoring carries
  on without it.
- **`KeepAliveWorker`** (WorkManager) and **`BootReceiver`** bring things back after the process
  is killed or the device reboots.
- **The Rust bridge** (`rust/src/lib.rs`) holds one `Daemon` for the process. Every `native*`
  JNI call is a method call on it. `nativeRunDaemonLoop` and `nativeStopDaemon` share a
  loop state, so a restart waits for a previous loop that is still flushing instead of failing
  (see `client/core/architecture.md`).

Capture used to use MediaProjection and moved to the Accessibility service in #386.
MediaProjection can't be granted silently, some devices ask again after a reboot or after the
process is killed, and on Android 14+ each grant works only once. None of that fits unattended
monitoring. `ProjectionPermissionStore.kt` is left over from that and is no longer used.

## Self-update

Stable release builds update themselves (`AppUpdater.kt`). Dev, staging, and debug builds
don't: the `AUTO_UPDATE` BuildConfig flag is only on for release builds whose channel is
`stable` (`VIRTUE_RELEASE_CHANNEL`, or `GITHUB_REF_NAME=main` in CI), so a tester's staging
build never replaces itself with the stable APK.

- A WorkManager job checks `https://virtueinitiative.org/android-update.json` every 6 hours,
  and once when the app opens, on unmetered networks only. The landing site writes that
  feed at build time (`landing/scripts/build-android-update.mjs`) from the latest stable
  GitHub release: its version, APK URL, and the SHA-256 GitHub reports for the APK. `{}`
  means no update.
- An update is offered when the feed's version is above `VERSION`. The APK is downloaded to
  the app's private `no_backup/updates` directory, its SHA-256 checked against the feed, and its package name and
  versionCode checked against the installed app. Stable releases always raise
  `ANDROID_VERSION_CODE` (`check-version-bump.sh`). Android rejects an APK signed with a
  different key.
- It installs through a `PackageInstaller` session. After a manual sideload, the system
  package installer is the installer of record, so the first update needs the user: a
  notification and an **Install Update** button in the status card start the session from
  the foreground (a background app can't open Android's confirmation dialog). If "Install
  unknown apps" is off for Virtue, Android's dialog sends the user to that switch and comes
  back on its own (Android 12+; on 10 and 11 the user presses Back). After that, Virtue is
  its own installer of record, and Android 12+ installs later updates silently
  (`USER_ACTION_NOT_REQUIRED` + `UPDATE_PACKAGES_WITHOUT_USER_ACTION`). Android 10 and 11
  always ask. A silent install waits while the app is on screen.
- Updating keeps Accessibility on; the service reconnects in the new process about a second
  after the install.
- A self-update resets the install source, which also lifts Android's restricted-settings
  block on Android 15+ but not on 13 and 14. `AccessibilitySetupGuide` remembers the
  first-launch install source so the unlock steps still show there.

### Testing an update locally

Build two APKs with `-PautoUpdate=true
-PupdateManifestUrl=http://localhost:8765/android-update.json` and different
`-PversionCodeOverride` values, serve a feed pointing at the higher one (with a
version above `VERSION`) over `adb reverse tcp:8765 tcp:8765`, and temporarily allow
cleartext traffic in the manifest. Install the lower one through Chrome rather than `adb
install` to reproduce a real sideload.
