# Virtue iOS Client (Safari Extension Capture + Xcode)

This iOS client now captures screenshots **only from Safari** using a Safari Web
Extension. ReplayKit/system broadcast is removed.

## Architecture

- iOS app (`VirtueIOS`): login/session/runtime override UI + native core init.
- Safari Web Extension (`VirtueSafariWebExtension`):
  - JS captures the visible Safari tab image.
  - Native extension handler stores the latest PNG in-memory.
  - Rust daemon loop runs in the extension process and samples via the same C
    capture callbacks when `run_batch_daemon` asks.
- Shared App Group storage (`group.org.virtueinitiative.virtueios`) carries:
  - token/state files for Rust core
  - runtime overrides
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
- Default overrides are hardcoded at startup:
  - `VIRTUE_BASE_API_URL=http://10.7.7.4:8787`
  - `VIRTUE_CAPTURE_INTERVAL_SECONDS=15`
  - `VIRTUE_BATCH_WINDOW_SECONDS=30`

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
