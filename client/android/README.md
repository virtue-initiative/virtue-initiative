# Virtue Android client

A Kotlin app whose Accessibility service runs the shared Rust core through JNI and takes the
screenshots. It is sideloaded as an APK and updates itself. What it does for users is specified
in `client/SPEC.md` (CLIENT-030 and CLIENT-031).

- [DESIGN.md](DESIGN.md): what runs where, and how self-update works.

## Layout

- `app/src/main/java/org/virtueinitiative/virtue/`: the app. `VirtueAccessibilityService.kt`
  runs the daemon loop and captures, `MainActivity.kt` is the UI, `NativeBridge.kt` declares the
  JNI calls, `AppUpdater.kt` is the self-updater.
- `rust/`: the JNI bridge crate (`android/rust/src/lib.rs`), built by Gradle through
  `cargo-ndk`.
- `scripts/`: emulator and build helpers, wrapped by the `just android-*` recipes.

## One-time setup

You need JDK 17, the Android SDK in `~/Android/Sdk` (or set `ANDROID_SDK_ROOT`), Rust with the
Android targets, and `cargo-ndk`. Install the SDK pieces the project expects (`compileSdk` and
`targetSdk` 35, NDK `26.1.10909125`):

```bash
yes | sdkmanager --licenses
sdkmanager "platform-tools" "emulator" "platforms;android-35" "build-tools;35.0.0" \
  "system-images;android-35;google_apis;x86_64" "ndk;26.1.10909125" "cmdline-tools;latest"
```

Create the emulator the scripts use by default, `virtue_api35`:

```bash
avdmanager create avd -n virtue_api35 -k "system-images;android-35;google_apis;x86_64" -d pixel_7 --force
```

`./client/android/scripts/doctor.sh` checks the toolchain and emulator. The scripts read SDK
paths and the AVD name from `scripts/env.sh`, which you can override in
`~/.config/virtue-dev.env` or the repo-root `.env` (see `docs/development.md`).

## Build and run

```bash
just android-start     # start the emulator and wait for boot (headless=true for no window)
just android-install   # build, install and launch the debug app
just android-logs      # app logs (follow=true to tail)
just android-lint      # lint, the closest thing to a typecheck
```

`just android-build` only builds; the APK lands in `app/build/outputs/apk/debug/`. Pass
`release=true` to any build recipe for a release build. `just android-lock` and
`just android-force-idle` lock the screen and force Doze, for testing that monitoring survives
in the background.

After installing, turn on the "Virtue Screen Monitor" Accessibility service when the app asks.
It won't show the sign-in form until the service is on.

`docs/ci-checks.md` lists the exact Gradle tasks CI runs.

## Pointing it at a local API

Set `VIRTUE_DEFAULT_API_URL` in the repo-root `.env` and rebuild. See
[`client/README.md`](../README.md). Inside the emulator, the host is `10.0.2.2`, so use
`http://10.0.2.2:<port>`, not `localhost`. If the API runs on another machine, start it on all
interfaces (see `api/README.md`).

## Releases

CI (`.github/workflows/client-android.yml`) builds a signed release APK from the
`ANDROID_KEYSTORE_BASE64` and `ANDROID_KEYSTORE_PASSWORD` secrets and attaches it to the GitHub
release. Only stable builds update themselves (see [DESIGN.md](DESIGN.md#self-update)). Android
refuses an update signed with a different key, so the keystore must never change.
