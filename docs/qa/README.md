# QA Checklists

Manual QA checklists for each Virtue client platform. Run these before tagging a release.

## Platform checklists

| Platform | File | Notes |
|---|---|---|
| macOS | [mac.md](mac.md) | Tray app + LaunchAgent daemon |
| Windows | [windows.md](windows.md) | WinUI 3 + MSIX + Rust DLL |
| Linux | [linux.md](linux.md) | CLI + systemd service + optional tray |
| Android | [android.md](android.md) | Foreground service + MediaProjection |
| iOS | [ios.md](ios.md) | Safari Web Extension capture only |

## Shared baseline

Items common to all platforms (auth, upload, hash chain, retry, lifecycle logs) are listed in each platform checklist rather than a separate shared doc so each checklist stands alone.

## Conventions

- `[ ]` — unchecked item
- Mark items `N/A` when a feature genuinely does not apply to the build under test
- Record test environment (OS version, app version, API env: prod/staging/local) at the top of each run
- For upload and hash-chain tests, verify behavior in the web app at https://app.virtueinitiative.org (or your staging URL)
