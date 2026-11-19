# Virtue Linux client

A `virtue` command, a systemd user service that runs the shared Rust core, and a tray icon,
shipped as a `.deb` for Debian and Ubuntu. What it does for users is specified in
`client/SPEC.md` (CLIENT-020 to CLIENT-023), including every `virtue` subcommand.

- [DESIGN.md](DESIGN.md): how the `.deb` is built and how auto-update works.

## Layout

- `src/main.rs`: the `virtue` command. `daemon.rs` runs the core's loop and IPC server,
  `tray.rs` the tray icon, `capture.rs` the screenshot backends.
- `packaging/`: the Debian maintainer scripts, the systemd units and the update-check script.
- `scripts/build-deb.sh`, `scripts/build-deb-docker.sh`: build the `.deb`.
- `tests/`: tests for the `status` output and the maintainer scripts.

## Build and install

```bash
just linux-test      # cargo test -p virtue-linux
just linux-build     # .deb in client/target/debian/
just linux-install   # build, then sudo dpkg -i it
```

`just linux-build` runs `scripts/build-deb.sh`, which builds natively. It needs
`libleptonica-dev`, `libtesseract-dev`, `libclang-dev`, `clang` and `patchelf`. It takes
`--debug` for a faster debug build, and `--instance <name>` for a side-by-side install (below).

Release `.deb`s are built in Docker instead, so they run on older distributions too:

```bash
cd client && ./linux/scripts/build-deb-docker.sh   # .deb in client/target-docker/debian/
```

[DESIGN.md](DESIGN.md#building-the-deb) explains the difference.

After installing, run `virtue login` inside your desktop session. The service starts for every
logged-in user when the package is installed, and idles with capture off until you sign in.

## Side-by-side installs

`build-deb.sh --instance <name>` builds a package named `virtue-<name>`, with its own binary,
service, tray id and config and state directories (CLIENT-002). It can be installed next to a
regular `virtue`, which is useful for testing a branch against a different API without touching
your main install. Instance builds never get the auto-update timer.

## Pointing it at a local API

Set `VIRTUE_DEFAULT_API_URL` in the repo-root `.env` and rebuild. See
[`client/README.md`](../README.md). `virtue status --advanced` prints the values a build was
made with.

## Integration test

```bash
bun client/linux/scripts/integration-test.ts
```

It starts a local API, runs the real daemon under Xvfb, signs in as the seeded dev account and
checks that hashes and batches reach the database. It needs `bun`, `cargo`, `curl` and
`xvfb-run` (`apt-get install xvfb`), and runs with its own temporary `HOME` and XDG directories,
so it doesn't touch a real install. `scripts/ci-login.ts` answers `virtue login`'s password
prompt through `script(1)`.

## Files on a running machine

| Path                                     | What                                       |
| ---------------------------------------- | ------------------------------------------ |
| `~/.local/state/virtue/event_state.json` | The core's state (CORE-016)                |
| `~/.config/virtue/`                      | Config, including the remembered email     |
| `journalctl --user -u virtue.service`    | The daemon's log                           |
| `/usr/lib/virtue/`                       | Build label, release tag and update script |

`XDG_STATE_HOME` and `XDG_CONFIG_HOME` move the first two when set.

## Troubleshooting

- **`virtue login` says capture is failing.** On X11, install ImageMagick (`import`) or `maim`.
  On Wayland, capture needs a compositor that lets `grim` take screenshots. Otherwise use an X11
  session.
- **No tray icon.** The desktop needs a StatusNotifier host. Monitoring runs without one.
- **Not monitoring.** `virtue status --full` shows recent errors.
  `systemctl --user status virtue.service` shows whether the service is running.
- **Updates.** `systemctl status virtue-update.timer` and `journalctl -u virtue-update.service`.
