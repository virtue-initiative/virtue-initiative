# Linux client design

Why the Linux-specific parts work the way they do. The behavior is specified in
`client/SPEC.md` (CLIENT-020 to CLIENT-023). For building and running, see
[README.md](README.md).

## Process model

The daemon (`virtue daemon`) runs as the systemd user service `virtue.service`, one per
logged-in user. It runs the core's loop and the core's IPC server (CORE-025), and starts the
tray icon if a tray host is available. Every other `virtue` subcommand is a short-lived client
that talks to the daemon over that socket.

## Building the `.deb`

Both build scripts bundle `libtesseract`/`liblept`/`libjpeg` into the package (instead of depending
on the OS-provided packages, whose names — and in libjpeg's case, ABI — vary across distro
versions) and uses `patchelf` to set their RPATH. It also bundles the `eng.traineddata`
Tesseract language data file so OCR-based screenshot redaction works out of the box, without
depending on the OS `tesseract-ocr-eng` package.

### Why the release build uses Docker

`scripts/build-deb-docker.sh` builds inside a container pinned to Debian **oldstable** (bookworm, see
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

### Native builds

`scripts/build-deb.sh` builds directly on the host and is faster for local iteration, but the resulting `.deb`'s
`Depends:` versions are only as old/compatible as whatever distro you're running. The output
`.deb` is created under `target/debian/`. Requires `libleptonica-dev`, `libtesseract-dev`,
`libclang-dev`, `clang`, and `patchelf` (`sudo apt-get install patchelf` if it isn't already
present).

If you prefer `cargo deb`, the crate includes metadata for it, but the scripts above have no
extra Rust tool dependencies beyond `patchelf`.

## Auto-update

The package installs a system-level (not `--user`) `virtue-update.timer` /
`virtue-update.service` pair. The timer fires 10 minutes after boot and then every 6 hours
(± a random 30-minute delay), running `/usr/lib/virtue/update-check.sh` as root.

Whether `postinst` actually enables/starts the timer is controlled by
`/usr/lib/virtue/auto-update-enabled` (`"1"` or `"0"`), baked into the package at build time by
`VIRTUE_ENABLE_AUTO_UPDATE=1 ./scripts/build-deb-docker.sh`. This defaults to **off** — a
native/Docker build run locally, or a PR's `--debug` CI build, never enables the timer, so it
can't silently overwrite a developer's local install with whatever's on GitHub Releases. Only
the CI job that actually publishes a release build to `main`/`staging` sets the env var, so only
an officially-distributed `.deb` ships with auto-update turned on by default.

The script lists recent GitHub Releases and follows the channel of the build that is installed.
The channel comes from the release tag baked into the package at build time
(`/usr/lib/virtue/release-tag`: `<VERSION>` on the `main`-branch/stable channel, `<VERSION>-dev` on
every other branch/dev channel; see `client/scripts/version.sh`). A `-dev` tag follows the newest
prerelease and any other tag follows the newest full release, so an install keeps updating across
version bumps, which publish to a new tag. Releases without a `.deb` for this architecture yet are
skipped. If the chosen release's `.deb` has a different embedded build label than the locally
installed one (`/usr/lib/virtue/build-label`), it downloads and `dpkg -i`s it, falling back to
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
