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

## Local API override (same 3 vars as Linux)

The Android app supports these runtime overrides:

- `VIRTUE_BASE_API_URL`
- `VIRTUE_CAPTURE_INTERVAL_SECONDS`
- `VIRTUE_BATCH_WINDOW_SECONDS`

Set them in the login screen under "Runtime overrides (optional)" and tap `Save overrides`.
Values are persisted and applied to the native core immediately.

Important for emulator networking:

- Use `http://10.0.2.2:8787` to reach an API running on your host machine at `localhost:8787`.
- Do not use `http://localhost:8787` inside the emulator (that points to the emulator itself).

APK output path (if you want manual install):

- `client/android/app/build/outputs/apk/debug/app-debug.apk`

Manual install alternative:

```bash
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

## Verify app is running

```bash
adb shell pm list packages | grep org.virtueinitiative.virtue
adb shell pidof -s org.virtueinitiative.virtue
adb logcat --pid "$(adb shell pidof -s org.virtueinitiative.virtue)"
```
