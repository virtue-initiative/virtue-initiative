# Virtue iOS Client (Safari Extension Capture + Xcode)

This iOS client now captures screenshots **only from Safari** using a Safari Web
Extension. ReplayKit/system broadcast is removed.

## Architecture

- iOS app (`VirtueIOS`): login/session UI + native core init.
- Safari Web Extension (`VirtueSafariWebExtension`):
  - JS captures the visible Safari tab image.
  - Native extension handler stores the latest PNG in-memory.
  - Rust daemon loop runs in the extension process and samples via the same C
    capture callbacks when `run_batch_daemon` asks.
- Shared App Group storage (`group.org.virtueinitiative.virtueios`) carries:
  - token/state files for Rust core
  - Safari capture heartbeat/status for the app UI

## Layout

- `app/Sources/`
  - `VirtueIOSApp.swift`: SwiftUI app + login/settings/status UI.
  - `MonitoringCoordinator.swift`: app orchestration and Safari extension status.
  - `NativeBridge.swift`: wrappers for Rust exported functions.
- `app/SafariWebExtension/`
  - `SafariWebExtensionHandler.swift`: native handler + daemon + capture callbacks.
  - `Resources/manifest.json`: extension manifest.
  - `Resources/background.js`: capture + native message bridge.
  - `Resources/content.js`: page-side capture tick trigger.
  - `Info.plist`: extension manifest (`com.apple.Safari.web-extension`).
  - `VirtueSafariWebExtension.entitlements`: app group entitlement.
- `app/Shared/`
  - `VirtueShared.swift`: shared keys/defaults/constants.
- `rust/`
  - `src/lib.rs`: Rust bridge for init/login/logout/run daemon.

## Runtime behavior

1. Launch app and sign in.
2. In iOS Settings, enable **Virtue Safari Capture** under Safari extensions.
3. Allow extension access to **All Websites**.
4. Browse in Safari.
5. Extension captures visible-tab screenshots and keeps only latest frame.
6. Rust daemon samples that latest frame based on configured intervals.

## Notes

- Capture is Safari-only; non-Safari apps are not captured.
- Capture depends on extension enablement and active Safari browsing context.
- `api_base_url`, `capture_interval_seconds`, and `batch_window_seconds` are
  compile-time constants baked into `virtue-core` via `env!()` (see
  `client/core/build.rs`) — there is no runtime override mechanism on any
  platform. To use local dev values, copy `.env.example` (repo root) to
  `.env` (gitignored) and set `VIRTUE_DEFAULT_API_URL`,
  `VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS`, `VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS`
  before building.

## Generate project

```bash
cd client/ios
./scripts/generate-project.sh
```

## Simulator build

```bash
cd client/ios
./scripts/build-ios.sh --destination "generic/platform=iOS Simulator"
```

## Run on connected iPhone

```bash
cd client/ios
./scripts/run-on-device.sh --team-id <APPLE_TEAM_ID>
```
