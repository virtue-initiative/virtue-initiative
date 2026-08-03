# `va` — Virtue Android dev CLI

A friendly wrapper around Gradle + the Android emulator for day-to-day work on
the Android client.

```bash
client/android/scripts/cli/va <command>
```

Tip: add it to your PATH for the session — `export PATH="$PWD/client/android/scripts/cli:$PATH"` — then just `va install`.

## Commands

| Command         | What it does                                          |
| --------------- | ----------------------------------------------------- |
| `va install`    | Build, install, and launch the app on the emulator    |
| `va build`      | Build the APK (+ Rust JNI core) only                  |
| `va start`      | Start the emulator and wait for boot                  |
| `va stop`       | Kill the running emulator                             |
| `va lock`       | Lock the device screen                                |
| `va force-idle` | Force the device into Doze (test background survival) |

### Flags

- `--release` — use the release build variant (`install` / `build`)
- `--headless` — run the emulator with no window (`start`)
- `-h`, `--help` — full usage

### Typical loop

```bash
va start            # boot the emulator (--headless for no window)
va install          # build + install + launch
va force-idle       # exercise Doze / background survival
va stop             # tear down
```

## Configuration — `env.sh`

`env.sh` is committed and only sets defaults. Resolution order (later wins):

1. Defaults in `env.sh`
2. `scripts/cli/env.local.sh` (untracked — your machine paths; see `env.local.sh.example`)
3. Variables already exported in your shell

For machine-specific paths (e.g. a storage-redirected SDK), copy
`env.local.sh.example` to `env.local.sh` — it's git-ignored.

Key vars: `ANDROID_SDK_ROOT`, `ANDROID_NDK_ROOT`, `VIRTUE_AVD`,
`VIRTUE_PACKAGE`, `VIRTUE_ACTIVITY`. To point at a different emulator for one
run: `VIRTUE_AVD=my_avd va start`.
