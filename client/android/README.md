# BePure Android

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
- Android cmdline tools (`sdkmanager`, `avdmanager`)
- Rust + android targets
- `cargo-ndk`

Validate:

```bash
./client/android/scripts/doctor.sh
```

## Build debug APK

From `client/android`:

```bash
./gradlew :app:assembleDebug
```

APK path:

- `client/android/app/build/outputs/apk/debug/app-debug.apk`

## Run on emulator

```bash
emulator -avd Medium_Phone_API_35
adb install -r client/android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n codes.anb.bepure/.MainActivity
```

