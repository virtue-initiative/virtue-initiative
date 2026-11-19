# Releasing the macOS client

CI builds, signs, notarizes and publishes the macOS app from `.github/workflows/client-macos.yml`.
This page covers how updates reach users and the signing setup it depends on. For local builds,
see [README.md](README.md).

## Turning auto-update on

Auto-update is off unless the app was built with `VIRTUE_ENABLE_AUTO_UPDATE=1`, which bakes
`SUFeedURL`/`SUPublicEDKey` into `Info.plist`; an empty feed URL means `UpdateController`
never starts the updater at all. This mirrors the Linux package's
`/usr/lib/virtue/auto-update-enabled` flag, and means a locally built app can never replace
itself with whatever is currently on GitHub Releases. Only the release-branch CI job sets it,
and only when a real Developer ID signing identity was available — Sparkle refuses an update
not signed by the same team, so an ad-hoc build would ship an updater that could never
install anything.

## Versions and channels

Sparkle orders updates by `CFBundleVersion`, which for this app is `<VERSION>.<commit-minutes>`
(e.g. `0.1.0.29796620`) — see `virtue_mac_bundle_version` in `client/scripts/version.sh`. The
fourth component is the commit timestamp in minutes, computed at build time, so dev-channel
builds between version bumps still compare correctly and **nothing has to be regenerated or
committed** for an update to be offered. It is deliberately not `APPLE_BUILD_NUMBER`: that
value is shared with iOS, where App Store submission caps `CFBundleVersion` at three integer
components.

One feed at `https://virtueinitiative.org/appcast.xml` serves both channels. Stable items are
untagged; dev items carry `<sparkle:channel>dev</sparkle:channel>`, which only dev builds opt
into via `SPUUpdaterDelegate.allowedChannels`. The feed is assembled by
`landing/scripts/build-appcast.mjs` from per-release `appcast-item-macos.xml` fragments that
`mac/scripts/make-appcast-item.sh` signs on the macOS runner. GitHub Releases can't host the
feed itself: a `releases/latest/download/<file>` URL skips prereleases, and a per-tag URL never contains
anything newer than the build it shipped with.

## Signing Sparkle

`build-app.sh` signs Sparkle's nested helpers (`Autoupdate`, `Updater.app`, and the two XPC
services) explicitly, inside out, before signing the framework and the app. `xcodebuild`
signs the outer framework when embedding it but leaves those helpers carrying the ad-hoc
signature from the SPM binary artifact, which passes `codesign --verify --deep --strict`
locally and then fails notarization. The build asserts the team identifier on each nested
binary so this can't regress silently. The outer re-sign is deliberately **not** `--deep`,
which Sparkle documents as breaking its updater and Apple deprecates.

## Renewing the Developer ID certificate

The release certificate is a "Developer ID Application: Virtue Initiative Inc. (Y2Z8ZS4D33)"
certificate. Renewing it needs no code change, because the Team ID — which TCC and Sparkle key
on — stays the same. Do this when it nears expiry, and whenever Apple retires a Developer ID
intermediate (the original Sub-CA expired 2027-02-01; certificates must come from **G2**):

1. Create a CSR in Keychain Access (or with `openssl`) on the Mac that will hold the private key.
2. In Certificates, Identifiers & Profiles (Account Holder only), add a **Developer ID
   Application** certificate, choose the **G2 Sub-CA** intermediary, and upload the CSR.
3. Install the downloaded `.cer` and check `openssl x509 -inform DER -in <file> -noout -issuer`
   shows `OU=G2`.
4. Update the `MACOS_DIST_CERTIFICATE_P12_BASE64` and `MACOS_DIST_CERTIFICATE_PASSWORD` GitHub
   secrets with a `.p12` export of the new identity (certificate and private key). CI signs and
   notarizes with whatever that secret holds, so releases keep using the old certificate until it
   is replaced.
5. Nothing to change locally: with no `CODESIGN_IDENTITY` set, `build-app.sh` signs with the
   team's valid Developer ID certificate that expires last, so the old and new certificate can
   sit side by side until the old one is deleted. Set `CODESIGN_IDENTITY` (a name or SHA-1 from
   `security find-identity -v -p codesigning`) to force a specific one.
