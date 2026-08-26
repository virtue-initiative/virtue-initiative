# Virtue Linux Client

## Commands

- `virtue login`
  - Prompts for email/password.
  - Registers a device with the API and stores `device_id`.
- `virtue logout`
  - Warns that a log event is sent to indicate monitoring was turned off.
  - Clears local auth/device state and disables monitoring.
- `virtue status`
  - Shows login and monitoring state.
- `virtue report-issue [--message ...] [--contact-email ...] [--yes]`
  - Prompts for a description (and optional contact email) and emails it to the Virtue Initiative team, attaching the last day of this device's `virtue` service logs (via `journalctl --user`). These are diagnostic logs only — no screenshots or screenshot content, no window titles — and any known secret/token patterns are redacted before sending.
  - Prints exactly what will be sent (message, contact email, platform details, log attachment) and asks for confirmation before submitting; `--yes` skips the prompt.
  - Works whether or not the daemon is running or the device is logged in; attaches this device's identity automatically when it's logged in.
- `virtue daemon`
  - Background worker used by systemd.
  - On desktop sessions, it also starts a minimal tray icon with hover status.
- `virtue daemon start`
  - Starts the `virtue.service` user unit through `systemctl --user`.
- `virtue daemon stop`
  - Stops the `virtue.service` user unit through `systemctl --user`.
  - Records an explicit user stop-intent before stopping so the lifecycle event is classified as user-requested.
- `virtue dev upload-log --risk 0.7 [--title ...] [--details ...]`
  - Sends a developer log immediately with the provided risk score.
- `virtue dev add-log --risk 0.7 [--title ...] [--details ...]`
  - Queues a metadata-only developer log into the next encrypted batch.
- `virtue dev add-screenshot --risk 0.7 [--title ...] [--details ...]`
  - Captures a screenshot and queues it into the next encrypted batch.
- `virtue dev upload-batch`
  - Forces the currently queued batch items to upload now.

## Service behavior

The service is installed and auto-started for active desktop users by the package `postinst` script.
Before `virtue login`, monitoring is idle because there is no token/device binding.
After `virtue login`, captures and uploads start automatically.
The tray icon (when available) is started and stopped by the daemon process.
If a tray host is unavailable at daemon startup, monitoring continues without the tray icon.
Linux lifecycle logs include:

- `system_login` when a new login session is observed (OS session/user login, including a fresh boot).
- `system_logout` at the end of an expected-running window (OS session/user logout).
- `suspend_detected` for a suspend interval found retrospectively via boot-vs-monotonic clock divergence.

And lifecycle alerts, fired when the expected login→logout running window doesn't match what was actually observed:

- `unexpected_start` when the process wasn't running during a stretch of awake time between a known login and the first observed heartbeat.
- `unexpected_stop` when the process stopped running before the session's logout.
- `unexpected_gap` for a stretch of awake time (same boot) between two heartbeats with no sample — crash, force-kill-and-restart, or frozen process.
- `user_stop` when the user explicitly quit the monitor (e.g. `virtue daemon stop`) while it was expected to be running.

`system_logout` and `unexpected_stop` are best-effort: abrupt power loss, kernel panic, or very late shutdown network teardown can still prevent immediate delivery.

### Lifecycle Log Distro Support

- Officially supported install path: Debian/Ubuntu-family distributions using the packaged `.deb`.
- Lifecycle logs (`system_login` / `system_logout` / alerts) are supported on Linux distributions that use:
  - `systemd` (for service lifecycle and shutdown-state detection), and
  - procfs with `/proc/sys/kernel/random/boot_id` (startup detection).
- Non-systemd distributions are not currently supported for system lifecycle logs.

`api_base_url`, `capture_interval_seconds` (default `300`, minimum `15`), and
`batch_window_seconds` (default `3600`, minimum `1`) are baked into the binary
at **compile time** — there is no runtime override file. To build against a
local API or with different intervals, set `VIRTUE_DEFAULT_API_URL`,
`VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS`, and/or
`VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS` (as real env vars, or via a
`client/.env` file copied from `client/.env.example`) before running
`cargo build`.

`virtue status` prints the current build-resolved values.

The client uses `XDG_CONFIG_HOME` and `XDG_STATE_HOME` when those variables are set. Otherwise it falls back to `~/.config/virtue` for config and `~/.local/state/virtue` for mutable state.

## Auto-update

The package installs a system-level (not `--user`) `virtue-update.timer` /
`virtue-update.service` pair, enabled and started by `postinst`. The timer fires 10 minutes
after boot and then every 6 hours (± a random 30-minute delay), running
`/usr/lib/virtue/update-check.sh` as root.

The script polls the GitHub Releases API for the release tag baked into the package at build
time (`/usr/lib/virtue/release-tag` — `<VERSION>` on the `main`-branch/stable channel,
`<VERSION>-dev` on every other branch/dev channel, matching whichever channel this exact build
was produced from; see `client/scripts/version.sh`). If the release's current `.deb` asset for
this architecture has a different embedded build label than the locally installed one
(`/usr/lib/virtue/build-label`), it downloads and `dpkg -i`s it, falling back to
`apt-get install -f -y` on a dependency failure. Installing the new `.deb` re-runs `postinst`,
which restarts `virtue.service` for every logged-in user — the same upgrade path as a manual
`dpkg -i`. A `flock` on `/run/lock/virtue-update.lock` prevents overlapping runs; a failed run
exits non-zero and is simply retried at the next timer firing.

Check status/logs with:

```bash
systemctl status virtue-update.timer
journalctl -u virtue-update.service
```

`--instance`-suffixed side-by-side builds (used for local dev/testing) do not get this timer,
matching how they also skip `postinst`/`prerm`/the regular systemd unit install.

## Wayland and X11

`virtue login` runs a capture probe.

- On X11, install `imagemagick` (`import`) or `maim` if capture tools are missing.
- On Wayland, unattended capture support depends on compositor permissions.
  - Recommended for reliability: use an X11 session for monitoring.
  - Alternative: compositor-specific setup that permits `grim` screencopy.

## Build .deb

The script bundles `libtesseract`/`liblept`/`libjpeg` into the package (instead of depending
on the OS-provided packages, whose names — and in libjpeg's case, ABI — vary across distro
versions) and uses `patchelf` to set their RPATH. It also bundles the `eng.traineddata`
Tesseract language data file so OCR-based screenshot redaction works out of the box, without
depending on the OS `tesseract-ocr-eng` package.

### Recommended: Docker build (widest compatibility)

From the `client/` workspace root:

```bash
./linux/scripts/build-deb-docker.sh
```

This builds inside a container pinned to Debian **oldstable** (bookworm, see
`linux/docker/Dockerfile`). Building against an older glibc/libstdc++/system-library set is
forward-compatible — the resulting binary runs fine on newer Debian/Ubuntu releases, just not
older ones — so this is what CI uses to produce the release `.deb`. Bookworm was chosen
deliberately over older releases: Debian 10 (buster) is EOL with archived, unreachable apt
repos, and Debian 11 (bullseye) ships `libtiff5`/`libwebp6`, package names that no longer exist
on current Debian/Ubuntu (renamed to `libtiff6`/`libwebp7`), which would reproduce the exact
"depends on a renamed package" bug this bundling approach exists to fix. Bump the base image in
`linux/docker/Dockerfile` if Debian's oldstable moves to a new release.

The output `.deb` is created under `target-docker/debian/` (kept separate from `target/` since
build artifacts are tied to the glibc/rustc that produced them and can't be shared between a
host build and a container build of a different Debian release).

Only Docker itself is required locally; the container has its own Rust toolchain and system
dependencies.

### Alternative: native build

```bash
./linux/scripts/build-deb.sh
```

Builds directly on the host and is faster for local iteration, but the resulting `.deb`'s
`Depends:` versions are only as old/compatible as whatever distro you're running. The output
`.deb` is created under `target/debian/`. Requires `libleptonica-dev`, `libtesseract-dev`,
`libclang-dev`, `clang`, and `patchelf` (`sudo apt-get install patchelf` if it isn't already
present).

If you prefer `cargo deb`, the crate includes metadata for it, but the scripts above have no
extra Rust tool dependencies beyond `patchelf`.
