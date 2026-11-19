# Virtue Windows client

A WinUI 3 tray app that runs the shared Rust core in-process and ships through the Microsoft
Store as an MSIX package. What it does for users is specified in `client/SPEC.md` (CLIENT-027 to
CLIENT-029).

- [DESIGN.md](DESIGN.md): the process model, the restart watchdog and Store updates.
- [RELEASING.md](RELEASING.md): signing and Store submission.
- [VM_SETUP.md](VM_SETUP.md): creating the Windows VM used for builds from Linux.

## Layout

- `src/`: the `virtue-windows` Rust crate. `resident_monitor.rs` runs the daemon loop,
  `ffi.rs` is the C ABI the app calls, `capture.rs` takes screenshots with GDI.
- `Virtue.WindowsApp/`: the WinUI 3 app.
- `Virtue.WindowsApp.Core/`: interop, tray controller, view model and policies, kept free of
  WinRT so they can be unit-tested.
- `Virtue.WindowsApp.Tests/`: tests for `Virtue.WindowsApp.Core`.
- `scripts/build-msix.ps1`: builds and packages the MSIX on Windows.
- `scripts/remote-windows-build.sh`: runs the build in the VM from Linux.

## Build from Linux (the usual way)

Windows builds run in a Windows VM over SSH. Set the VM up once with [VM_SETUP.md](VM_SETUP.md),
which ends with an SSH alias such as `win11`. Then, from the repo root:

```bash
./client/windows/scripts/remote-windows-build.sh --build-host win11 --mode smoke
```

`--mode smoke` runs the CI checks: Rust build and clippy, the .NET build and the managed tests.
To build an installable package instead:

```bash
./client/windows/scripts/remote-windows-build.sh --build-host win11 --mode msix --profile Debug
```

Logs land in `client/windows/dist/remote-logs/`. Each worktree gets its own build directory in
the VM. If the first build in a new worktree recompiles most crates, run
`just cow-cache warm-windows` (see VM_SETUP.md, step 8). Run the script with `--help` for all
options.

[`scripts/vm-access.md`](scripts/vm-access.md) covers driving the VM: unlocking it, taking
screenshots, installing the MSIX and reading the log.

## Build on Windows

You need the Rust MSVC toolchain (`stable-x86_64-pc-windows-msvc`), the .NET 8 SDK, and Visual
Studio Build Tools with MSBuild, the desktop C++ and managed desktop workloads, and the Windows
10 SDK 19041 or later. `scripts/bootstrap-win11-build-vm.ps1` installs all of it.

```powershell
just windows-build          # MSIX package, Debug profile
just windows-test           # Rust tests for virtue-windows
```

The package and a setup zip land in `client/windows/dist/`. A local build is signed with a
self-signed development certificate, and the setup zip includes the `.cer` to trust before
installing. [RELEASING.md](RELEASING.md) lists every `build-msix.ps1` option.

`docs/ci-checks.md` lists the exact commands Windows CI runs.

## Pointing it at a local API

Set `VIRTUE_DEFAULT_API_URL` in the repo-root `.env` and rebuild. See
[`client/README.md`](../README.md). A VM reaches the host's API at the host's IP, not
`localhost`, so start the API on all interfaces (see `api/README.md`).

## Integration test

```bash
bun client/windows/scripts/integration-test.ts
```

Runs on Windows only. It starts a local API, runs the real library and checks that screenshots
reach the database. [DESIGN.md](DESIGN.md#integration-test) explains how it drives the library.

## Files on a running machine

Everything lives under `%PROGRAMDATA%\Virtue`, shared by every Windows account on the PC
(CLIENT-002):

| File                          | What                                         |
| ----------------------------- | -------------------------------------------- |
| `data\event_state.json`       | The core's state (CORE-016)                  |
| `data\logs\virtue.<date>.log` | The core's daily diagnostic log              |
| `config\ui_state.json`        | The remembered sign-in email                 |
| `ui-startup.log`              | The app's own log: startup and Store updates |

## Troubleshooting

- **No tray icon.** Launch Virtue from Start once, then check that the "Virtue Tray" startup
  app is on in Settings > Apps > Startup.
- **Signed in but not monitoring.** Read `data\logs\virtue.<date>.log` and Status Details in the
  app window.
- **The app didn't come back after a crash or update.** Check the watchdog task. See
  [DESIGN.md](DESIGN.md#watchdog).
- **An update never installs.** Read `ui-startup.log`. See
  [DESIGN.md](DESIGN.md#reading-the-update-log).
