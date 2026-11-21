# Releasing the iOS client

CI builds the iOS app and distributes it through TestFlight. This page covers that pipeline.
For local builds, see [README.md](README.md).

## TestFlight

`.github/workflows/client-ios.yml` archives, uploads and distributes on every push to
`main` and `staging`:

| Branch    | TestFlight group                                      | Beta App Review                   |
| --------- | ----------------------------------------------------- | --------------------------------- |
| `main`    | `Virtue Initiative iOS Public Beta` (external)        | required, submitted automatically |
| `staging` | whatever `IOS_STAGING_BETA_GROUP_NAME` names (opt-in) | skipped for internal groups       |

The job runs on GitHub's `xcode-27` image. App Store Connect requires builds made with the
iOS 27 SDK or later from April 2027, and `macos-latest` only has Xcode 26. Move it back to
`macos-latest` once that image defaults to Xcode 27 or later.

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

## Keeping stable builds out of the staging group

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

## Export compliance

A build with no export compliance answer sits in TestFlight as "Missing Compliance" and
reaches nobody. `ITSAppUsesNonExemptEncryption` is declared `false` in both the app and the
Safari extension `Info.plist`, so every build arrives already answered and the distribute
step only asserts it, failing the release if the key ever goes missing.

(The other way to satisfy this is an approved App Encryption Declaration attached per build
over the API. That is what you need if Apple ever _does_ require uploaded documents for
this app; it is deliberately not implemented while the Info.plist answer suffices.)

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

## "What to Test" notes

A pull request into `main` must have a `# What to Test (iOS)` section in its description. CI
fails without it, and the distribute step uses that text as the build's "What to Test" in
TestFlight. See "TestFlight What to Test" in `AGENTS.md`.
