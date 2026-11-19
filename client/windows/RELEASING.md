# Releasing the Windows client

CI builds and ships the Windows app from `.github/workflows/client-windows.yml`. This page
covers what it does and the one-time setup it depends on. For local builds, see
[README.md](README.md).

## Signing

GitHub Actions release packaging no longer uses an external signing service. The manifest's `Identity Publisher` (`client/windows/Virtue.WindowsApp/Package.appxmanifest`) must always match the publisher assigned by Partner Center for this app's Store identity, so the workflow lets `build-msix.ps1` fall back to its built-in self-signed development certificate (the same path used for PR/Debug builds) rather than overriding the publisher with an external signing cert's subject. Microsoft Store re-signs the package on ingestion, so a self-signed upload is sufficient for submission; for sideload installs the generated `.cer` in the setup bundle still needs to be trusted on the target machine as before.

## Store submissions

Two separate Store destinations, driven by branch:

| Branch    | Destination                  | Script                            | Who gets it         |
| --------- | ---------------------------- | --------------------------------- | ------------------- |
| `staging` | "Staging" package flight     | `scripts/submit-store-flight.ps1` | flight testers only |
| `main`    | the app's main Store listing | `scripts/submit-store-app.ps1`    | all Store customers |

Both run after the release MSIX is built and published, submit that same artifact via
the classic Store Submission API (`https://manage.devcenter.microsoft.com/v1.0/my/...`),
and are one-way pushes: each cancels any in-progress submission left over from a prior
run (whatever stage it is at — uncommitted, processing, or in certification — since any
of them block creating a new one), replaces the package, commits, and polls Partner
Center briefly to catch immediate validation failures before letting the CI job succeed.
Store certification and rollout continue asynchronously afterward, the same way the iOS
TestFlight upload step doesn't block on Apple's processing.

The two differ in more than their URLs:

- A **flight** submission is created empty; an **app** submission is created as a clone of
  the last published one, so the store listing, screenshots, age ratings and certification
  notes carry over and only the package is swapped. That clone is PUT back in full, which
  is why `submit-store-app.ps1` serializes it with `ConvertTo-Json -Depth 100` — past its
  depth limit `ConvertTo-Json` silently degrades nested nodes to strings rather than
  erroring, which would rewrite the live listing with garbage.
- The package list field is `applicationPackages`, not `flightPackages`.
- `submit-store-app.ps1` forces `targetPublishMode = Immediate` (and clears any cloned
  `targetPublishDate`) so a merge to `main` actually ships. Pass `-TargetPublishMode Manual`
  to stop after certification and publish by hand instead.

Package versions never collide between the two: `build-msix.ps1` gives stable (`main`)
builds `<VERSION>.0` and dev (`staging`) builds `<major>.<minor>.<GITHUB_RUN_NUMBER>.0`, so
flight packages stay ahead of the main listing as Microsoft's flighting model requires.

### One-time setup

This depends on one-time manual setup in Partner Center / Microsoft Entra ID that this
repo cannot provision on its own:

1. Register a Microsoft Entra ID application (Partner Center can create one directly from
   its "Microsoft Entra applications" management page) and generate a client secret for
   it in the Azure Portal (App registrations → Certificates & secrets).
2. In Partner Center, under Account settings → User management → Microsoft Entra
   applications, associate that app and grant it the Manager role on this developer
   account. (Managing this page requires being signed in as a Partner Center Manager
   who is also a Global Administrator of the Entra tenant.)
3. Look up the app's Store ID in Partner Center (App → App identity).
4. Create the "Staging" flight once in Partner Center (App overview → Package flights →
   New package flight), picking the tester group. Its Flight ID (a GUID) isn't surfaced
   directly in the UI — the reliable way to get it is to call the Store Submission API's
   `listflights` endpoint once the Entra app's credentials work:
   `GET https://manage.devcenter.microsoft.com/v1.0/my/applications/{STORE_APP_ID}/listflights`
5. Add the tenant/client/app/flight IDs as repo **variables** (not secrets — they aren't
   sensitive) and the client secret as a repo **secret**, read by
   `submit-store-flight.ps1` via matching env var names in `client-windows.yml`:
   - `WINSTORE_TENANT_ID` (variable) — Entra tenant ID
   - `WINSTORE_CLIENT_ID` (variable) — Entra app (client) ID
   - `WINSTORE_CLIENT_SECRET` (secret) — Entra app client secret
   - `WINSTORE_APP_ID` (variable) — Partner Center Store ID for this app (e.g. `9NXXXXXXXXXX`)
   - `WINSTORE_FLIGHT_ID` (variable) — GUID of the pre-created "Staging" flight
     (used only by the `staging` flight submission; the `main` app submission needs just
     the tenant/client/secret/app values above)

## `build-msix.ps1` options

- `-Profile Debug|Release` (default: `Debug`)
- `-Clean` (opt-in, only when you need a clean rebuild)
- `-CacheRoot C:\path\to\cache` (default: `%LOCALAPPDATA%\VirtueBuildCache`)
- `-SkipBuild` (reuse existing Rust artifacts and just rebuild/package the WinUI app)
- `-SkipSigning` (build/package the MSIX but leave the artifacts unsigned for external CI signing)
- `-PackagePublisher "CN=Publisher"` (override the MSIX manifest publisher during packaging)
- `-SigningCertificatePath C:\path\to\cert.pfx` (sign with a real code-signing certificate instead of the local dev cert)
- `-SigningTimestampUrl https://timestamp.example` (recommended for verified/release signing)

How signing works:

- Local builds without a configured PFX fall back to a self-signed development certificate and emit a `.cer` plus installer helper for sideloading.
- CI can build unsigned artifacts with `-SkipSigning` and then sign the generated `.msix` files externally.
- When a verified PFX is provided, the package manifest publisher is rewritten to match the certificate subject before signing. This must match exactly for MSIX installation to succeed.
- When signing outside the script, set `-PackagePublisher` or `VIRTUE_WINDOWS_PACKAGE_PUBLISHER` to the exact publisher subject that the external signer uses.
