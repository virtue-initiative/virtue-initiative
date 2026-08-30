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

## TestFlight releases

`.github/workflows/client-ios.yml` archives, uploads and distributes on every push to
`main` and `staging`:

| Branch    | TestFlight group                                      | Beta App Review                   |
| --------- | ----------------------------------------------------- | --------------------------------- |
| `main`    | `Virtue Initiative iOS Public Beta` (external)        | required, submitted automatically |
| `staging` | whatever `IOS_STAGING_BETA_GROUP_NAME` names (opt-in) | skipped for internal groups       |

`scripts/build-and-upload-testflight.sh` ends at `xcrun altool --upload-app`, which only
parks the build in App Store Connect — it processes for several minutes and then belongs
to no group. `scripts/distribute-testflight-build.mjs` does everything after that over the
App Store Connect API (altool cannot): wait for processing, record export compliance,
write "What to Test", attach the build to the named group, verify the attachment actually
took, and submit for Beta App Review when the group is external.

It runs under plain `node` with no dependencies — App Store Connect wants an ES256 JWT,
and `crypto.sign(..., { dsaEncoding: 'ieee-p1363' })` produces exactly the raw `r||s`
signature that needs.

If `IOS_STAGING_BETA_GROUP_NAME` is unset the step no-ops on `staging`, leaving that
branch's distribution to App Store Connect's own settings as before. `main` always
distributes.

### Keeping stable builds out of the staging group

`client/core/build.rs` bakes the API base URL in at compile time from the release channel:
a `main` build talks to `https://api.virtueinitiative.org`, a `staging` build to
`https://staging.app.virtueinitiative.org/api`. Group membership is therefore the only
thing keeping staging testers off production data, so on `main` the workflow sets
`IOS_EXCLUSIVE_BETA_GROUP=true` and the distribute step removes the build from every group
except the public beta, failing the job if it cannot.

This is a cleanup, not a guarantee. Internal groups with "automatically distribute new
builds" enabled pick up every build as soon as it becomes distributable, and that happens
asynchronously — so a tester could briefly see a stable build before it is pulled. The
setting is fixed when the group is created — App Store Connect does not let you change it
afterwards, and `hasAccessToAllBuilds` is read-only in the API too — so removal is the only
lever available. The step therefore prunes twice, once immediately after processing and
again at the end, to keep that window as short as possible.

To eliminate the window rather than shorten it, recreate the group with automatic
distribution off and set `IOS_STAGING_BETA_GROUP_NAME` so this workflow distributes staging
explicitly. External groups never auto-receive builds, so an external staging group has
nothing to prevent and this step will simply report the target group.

### Export compliance

A build with no export compliance answer sits in TestFlight as "Missing Compliance" and
reaches nobody, so the distribute step insists on one of two routes and fails loudly if
neither is present:

1. **`ITSAppUsesNonExemptEncryption` in `client/ios/app/Info.plist`.** The build arrives
   already answered and the step is a no-op. Deterministic, and what App Store Connect
   steers you to once it has told you your answers require no uploaded documents.
   **This is the route this repo takes** — the key is declared `false` in both the app and
   the Safari extension Info.plist.
2. **An approved App Encryption Declaration**, which the step finds and attaches per
   build. Set `IOS_APP_ENCRYPTION_DECLARATION_ID` to pin a specific one.

Note what "non-exempt" means in that key: _exempt from export **documentation**
requirements_, not "contains no cryptography". `client/core` implements AES-256-GCM, HPKE,
Argon2id and HKDF itself rather than calling Apple's CryptoKit, which by [Apple's
classification][export-compliance] is "industry standard algorithms not provided within
the Apple operating system" — that tier needs no CCATS, only a French declaration where
applicable, which is why App Store Connect reports that no documents are required.

Using exempt encryption without filing documentation with Apple can still carry a
year-end self-classification report obligation to the U.S. Bureau of Industry and
Security. That is a filing question, not a build question.

[export-compliance]: https://developer.apple.com/help/app-store-connect/reference/app-information/export-compliance-documentation-for-encryption/

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
