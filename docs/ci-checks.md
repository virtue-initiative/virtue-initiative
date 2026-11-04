# CI checks

The exact commands each GitHub Actions workflow runs, per component, so you can run the same
thing locally. `just --list` wraps the everyday ones. When a workflow's commands change, update
this page, the matching `just` recipe and `scripts/check-changed.ts`, which `just check` uses to
run these for every area a branch touches. For what to run while working, see
[`checking-changes.md`](checking-changes.md).

## Web/API CI (`.github/workflows/web.yml`)

Run this when touching `api/`, `api-donate/`, `web/`, `landing/`, `shared-web/`, or `theme.json`.

### API

From `api/`:

```bash
bun install
bun run typecheck
bun test
bun run format:check
```

Notes:

- `bun test` runs `vitest run`.
- API tests are documented in `api/TESTING.md`.

### Donate API

From `api-donate/`:

```bash
bun install
bun run typecheck
bun test
bun run format:check
```

Notes:

- `bun test` runs `vitest run` (Stripe calls are mocked via `cloudflare:test` `fetchMock`; webhook signatures are generated locally).
- Standalone Worker with separate `staging`/`prod` environments (own D1 database and route each), same pattern as `api/`. Deploys via `bun run deploy:staging` or `bun run deploy:prod`.
- Requires secrets `STRIPE_SECRET_KEY` and `STRIPE_WEBHOOK_SECRET` per Worker (`bunx wrangler secret put ... --name staging-donate|donate`, since `cf` can't set a single secret yet).

### Web app

From `web/`:

```bash
bun install
bun run typecheck
bun run test
bun run format:check
bun run build
```

### Landing site

From `landing/`:

```bash
bun install
GITHUB_TOKEN=stub bun run typecheck
bun run format:check
GITHUB_TOKEN=stub bun run build
```

Notes:

- CI provides `GITHUB_TOKEN` for landing checks because release metadata scripts may read GitHub releases.
- `typecheck` and `build` both prepare release data first.

### Shared web package

`shared-web/` does not currently have its own typecheck or test step in CI. If you change it, at minimum run:

From `shared-web/`:

```bash
bun run format:check
```

Also rerun the dependent checks in both `web/` and `landing/`, since that is where breakage will surface.

## Hash server (`.github/workflows/hash-server.yml`)

From `hash-server/`:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Spec and doc checks (`.github/workflows/spec-check.yml`)

From the repo root:

```bash
bun scripts/spec-check.ts                      # every test cites a spec ID (just spec-check)
bun scripts/doc-check.ts                       # docs only name files that exist (just doc-check)
bun scripts/check-deploy-secrets.ts --check-usage
```

## Client version check (`.github/workflows/version-check.yml`)

This only applies to pull requests into `main` that change client release versions.

CI compares `client/version.properties` on the PR branch against the base branch. To run the same script locally:

```bash
base_version_file="$(mktemp)"
git show <base-sha>:client/version.properties > "${base_version_file}"
./client/scripts/check-version-bump.sh "${base_version_file}" ./client/version.properties
```

Replace `<base-sha>` with the commit or branch you are targeting, for example `origin/main`.

## TestFlight "What to Test" (`.github/workflows/testflight-notes-check.yml`)

Pull requests into `main` must have a `# What to Test (iOS)` section in the PR body, with plain sentences for TestFlight testers. The check fails without it. The section is read by `client/scripts/extract-testflight-notes.sh`, which prints the text under the heading up to the next heading of the same or higher level. The `client-ios.yml` distribution step on `main` uses that text as the build's "What to Test". Staging builds keep the version label.

## Rust client workspace

The Rust workspace lives in `client/` (see `client/README.md`). Several members only compile on their own OS, so always name the package (`-p virtue-core`).

If you touch shared Rust code in `client/core/`, run the checks for every platform you can validate, because platform crates depend on it.

### Linux client CI (`.github/workflows/client-linux.yml`)

From `client/`:

```bash
cargo fmt --all -- --check
cargo clippy -p virtue-core --all-targets -- -D warnings
cargo clippy -p virtue-linux --all-targets -- -D warnings
cargo test -p virtue-core
cargo test -p virtue-core --features testing --tests
cargo test -p virtue-linux
./linux/scripts/build-deb.sh
```

Notes:

- This is the most complete Rust CI workflow and is the baseline for Linux or shared-core changes.
- `build-deb.sh` is the packaging step CI runs after tests.

### Linux client integration test (`.github/workflows/client-linux.yml`, `client-linux-integration` job)

End-to-end smoke test: starts the API against a fresh local D1 database, runs the real `virtue-linux` daemon under Xvfb, signs in as the seeded dev account and checks that hashes and batches land in the database. See `client/linux/README.md`.

```bash
bun client/linux/scripts/integration-test.ts
```

Requires `bun`, `cargo`, `curl`, `xvfb-run` on `PATH` (on Debian/Ubuntu: `apt-get install xvfb`). The Linux, macOS and Windows tests share `client/scripts/integration-test-lib.ts`. Android and iOS have none yet.

### macOS client CI (`.github/workflows/client-macos.yml`)

Run these on macOS.

From `client/`:

```bash
cargo build -p virtue-core
cargo build -p virtue-mac
cargo clippy -p virtue-core --all-targets -- -D warnings
cargo clippy -p virtue-mac --all-targets -- -D warnings
cargo test -p virtue-core
cargo test -p virtue-core --features testing --tests
(cd shared-swift && swift test)
./mac/scripts/build-dmg.sh
```

Notes:

- `build-dmg.sh` validates the app bundle and DMG packaging path.

### macOS client integration test (`.github/workflows/client-macos.yml`, `client-macos-integration` job)

Same smoke test, run against the real `virtue-mac` daemon binary with capture mocked, since CI has no Screen Recording grant. `client/mac/DESIGN.md` explains why that is safe.

```bash
bun client/mac/scripts/integration-test.ts
```

macOS only. Requires `bun`, `cargo`, `curl` on `PATH`.

### Windows client CI (`.github/workflows/client-windows.yml`)

Run these on Windows.

From `client/`:

```powershell
cargo build --target x86_64-pc-windows-msvc -p virtue-core
cargo build --target x86_64-pc-windows-msvc -p virtue-windows
cargo clippy --target x86_64-pc-windows-msvc -p virtue-core --all-targets -- -D warnings
cargo clippy --target x86_64-pc-windows-msvc -p virtue-windows --all-targets -- -D warnings
cargo test --target x86_64-pc-windows-msvc -p virtue-core
cargo test --target x86_64-pc-windows-msvc -p virtue-core --features testing --tests
cargo test --target x86_64-pc-windows-msvc -p virtue-windows
./windows/scripts/build-msix.ps1 -Profile Debug
```

Notes:

- From Linux, run the same checks in the Windows VM with `./client/windows/scripts/remote-windows-build.sh --build-host <vm> --mode smoke` (see `client/windows/README.md`).
- Release builds use `-Profile Release -Version <build_label>`, but debug packaging is the PR-time smoke test.

### Windows client integration test (`.github/workflows/client-windows.yml`, `client-windows-integration` job)

Same smoke test, driving the `virtue-windows` library through a small `virtue-windows-ci-runner` binary, since Windows has no separate daemon process. See `client/windows/DESIGN.md`.

```bash
bun client/windows/scripts/integration-test.ts
```

Windows only. Requires `bun`, `cargo` on `PATH`.

### Android client CI (`.github/workflows/client-android.yml`)

Run these on a machine with JDK 17, the Android SDK, NDK `26.1.10909125`, and the Rust Android targets installed (see `client/android/README.md`).

From `client/android/`:

```bash
./gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug :app:assembleDebug
```

Notes:

- CI also installs `cargo-ndk`.
- Release packaging in CI additionally runs:

```bash
./gradlew --no-daemon :app:assembleRelease
```

### iOS client CI (`.github/workflows/client-ios.yml`)

Run these on macOS with Xcode.

From `client/`:

```bash
cargo build -p virtue-core
```

From `client/ios/rust/`:

```bash
cargo build --manifest-path Cargo.toml
```

From `client/ios/`:

```bash
./scripts/build-ios.sh --destination "generic/platform=iOS Simulator" --derived-data .derived-data-ci-ios
```

Notes:

- CI's PR-time smoke test is the simulator build above.
- Release CI also builds an unsigned device app bundle:

```bash
./scripts/build-ios.sh --destination "generic/platform=iOS" --configuration Release --derived-data .derived-data-ci-ios-release --code-signing-allowed NO
```

### Swift style (`.github/workflows/swift-format.yml`)

Run this when touching any `.swift` file (`client/mac/`, `client/ios/`, `client/shared-swift/`).

From the repo root:

```bash
./scripts/swift-format.sh --check
```

Notes:

- Uses Apple's `swift-format` with the repo-root `.swift-format` config (4-space indent, 100 columns). It ships with Xcode 16+ and Swift 6 toolchains, so there's nothing extra to install, on macOS or Linux.
- `./scripts/swift-format.sh` (`just swift-format`) fixes formatting in place. `--check` (`just swift-format-check`) also fails on lint rules the formatter can't fix.
- It only looks at tracked files, so build output and checked-out packages are skipped.
- CI pins `swift:6.1.2`, because `swift-format`'s output can change between releases. If your local version formats differently from CI, CI's version wins.
- C FFI declarations (`@_silgen_name`/`@_cdecl`) keep their snake_case Rust symbol names, with `// swift-format-ignore: AlwaysUseLowerCamelCase` above each one.
