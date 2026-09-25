# Virtue Android

Android app scaffold with Kotlin UI + foreground screenshot service, using a Rust JNI layer that reuses `client/core` for auth, scheduling policy, queueing, retries, upload flow, and image pipeline.

## Structure

- `app/`: Android application module.
- `rust/`: Rust JNI bridge crate (`cdylib`) linked into the app.
- `scripts/doctor.sh`: local toolchain and emulator checks.

## Implemented behavior

- Login-only UI (email/password). No signup in app.
- After login:
  - Registers Android device with API (`POST /device`).
  - Prompts for MediaProjection capture permission.
  - Starts foreground monitoring service.
- Monitoring service:
  - Captures screenshots from MediaProjection virtual display.
  - Uses Rust core for interval scheduling + jitter/backoff.
  - Uses Rust core for image processing, queueing, retry, and upload.
  - Sends missed-capture style logs on failures.
- Sign out:
  - Sends a log indicating monitoring was turned off.
  - Clears auth/device state.
  - Stops service and returns to login UI.
- Aggressive background survival:
  - `START_STICKY` foreground service.
  - Restart attempt on task removal using `AlarmManager`.
  - Boot/package-replaced receiver restarts flow when possible.
  - Periodic WorkManager keepalive task.

## Important platform constraint

Android screen capture requires user-granted MediaProjection permission. There is no fully silent first-time permission grant. We persist projection data and try to restore it, but some OEMs/OS versions may still require re-granting after reboot/process loss.

## Build prerequisites

- JDK 21
- Android SDK + emulator in `~/Android/Sdk`
- Android command-line tools (`sdkmanager`, `avdmanager`, `adb`, `emulator`)
- Rust + Android targets
- `cargo-ndk`

From repo root, export SDK paths for this shell:

```bash
export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}"
export ANDROID_HOME="$ANDROID_SDK_ROOT"
export PATH="$ANDROID_SDK_ROOT/cmdline-tools/latest/bin:$ANDROID_SDK_ROOT/platform-tools:$ANDROID_SDK_ROOT/emulator:$PATH"
```

Run doctor check:

```bash
./client/android/scripts/doctor.sh
```

## One-time Android SDK setup

Install the exact SDK pieces this project expects (`compileSdk/targetSdk 35`, `ndkVersion 26.1.10909125`):

```bash
yes | sdkmanager --licenses
sdkmanager \
  "platform-tools" \
  "emulator" \
  "platforms;android-35" \
  "build-tools;35.0.0" \
  "system-images;android-35;google_apis;x86_64" \
  "ndk;26.1.10909125" \
  "cmdline-tools;latest"
```

## One-time emulator (AVD) creation

Create an emulator named `virtue_api35`:

```bash
avdmanager create avd \
  -n virtue_api35 \
  -k "system-images;android-35;google_apis;x86_64" \
  -d pixel_7 \
  --force
```

List available emulators:

```bash
emulator -list-avds
```

## Build, install, and run (every time)

From repo root:

1. Start emulator in background.

```bash
emulator -avd virtue_api35 -no-snapshot &
```

2. Wait until Android boot is complete.

```bash
adb wait-for-device
until adb shell getprop sys.boot_completed | tr -d '\r' | grep -q "^1$"; do sleep 1; done
adb shell input keyevent 82
```

3. Build and install debug app.

```bash
cd client/android
./gradlew :app:assembleDebug
./gradlew :app:installDebug
```

4. Launch app activity.

```bash
adb shell am start -n org.virtueinitiative.virtue/.MainActivity
```

## Local API URL / interval configuration

`api_base_url`, `capture_interval_seconds`, and `batch_window_seconds` are compile-time
defaults baked into the native core via the repo-root `.env` (see `.env.example`) — there is
no runtime override mechanism. Set `VIRTUE_DEFAULT_API_URL`,
`VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS`, and `VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS` in
`.env` at the repo root before building `client/android/rust` to point a local build at a dev API.

Important for emulator networking:

- Use `http://10.0.2.2:8787` to reach an API running on your host machine at `localhost:8787`.
- Do not use `http://localhost:8787` inside the emulator (that points to the emulator itself).

APK output path (if you want manual install):

- `client/android/app/build/outputs/apk/debug/app-debug.apk`

Manual install alternative:

```bash
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

## Automatic updates

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
  `no_backup/updates/`, its SHA-256 checked against the feed, and its package name and
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

To test locally, build two APKs with `-PautoUpdate=true
-PupdateManifestUrl=http://localhost:8765/android-update.json` and different
`-PversionCodeOverride` values, serve a feed pointing at the higher one (with a
version above `VERSION`) over `adb reverse tcp:8765 tcp:8765`, and temporarily allow
cleartext traffic in the manifest. Install the lower one through Chrome rather than `adb
install` to reproduce a real sideload.

## Verify app is running

```bash
adb shell pm list packages | grep org.virtueinitiative.virtue
adb shell pidof -s org.virtueinitiative.virtue
adb logcat --pid "$(adb shell pidof -s org.virtueinitiative.virtue)"
```
