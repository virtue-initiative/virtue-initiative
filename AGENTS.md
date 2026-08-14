# AGENTS.md

This repository is split across several independently-tested areas. When you change code, run the checks for every area you touched. If you change shared code or cross-cutting behavior, run all affected sections, not just the one you started in.

## General rules

- Prefer matching the existing GitHub Actions workflows in `.github/workflows/`.
- Use `bun install` for Node projects when you need a clean install matching CI.
- Use `cargo` commands from `client/` for Rust workspace checks unless a section below says otherwise.
- If you only change docs or non-executable assets, format checks may still be relevant if those files are covered by Prettier.
- Release packaging steps are listed below because CI runs them, but they are usually only needed when validating packaging or release changes.

## Repo-wide quick map

- `api/`: Cloudflare Workers API
- `api-donate/`: standalone donations Worker (Stripe Checkout)
- `web/`: main web app
- `landing/`: marketing site and help pages
- `shared-web/`: shared web assets used by `web` and `landing`
- `client/core/`: shared Rust core used by desktop/mobile clients
- `client/linux/`, `client/mac/`, `client/windows/`, `client/android/`, `client/ios/`: platform clients

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
- Requires secrets `STRIPE_SECRET_KEY` and `STRIPE_WEBHOOK_SECRET` per environment (`wrangler secret put ... --env staging|prod`).

### Web app

From `web/`:

```bash
bun install
bun run typecheck
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

## Client version check (`.github/workflows/version-check.yml`)

This only applies to pull requests into `main` that change client release versions.

CI compares `client/version.properties` on the PR branch against the base branch. To run the same script locally:

```bash
base_version_file="$(mktemp)"
git show <base-sha>:client/version.properties > "${base_version_file}"
./client/scripts/check-version-bump.sh "${base_version_file}" ./client/version.properties
```

Replace `<base-sha>` with the commit or branch you are targeting, for example `origin/main`.

## Rust client workspace

The Rust workspace lives in `client/` and includes:

- `virtue-core`
- `virtue-linux`
- `virtue-mac`
- `virtue-windows`

If you touch shared Rust code in `client/core/`, run the checks for every platform you can validate, because platform crates depend on it.

### Linux client CI (`.github/workflows/client-linux.yml`)

From `client/`:

```bash
cargo fmt --all -- --check
cargo clippy -p virtue-core --all-targets -- -D warnings
cargo clippy -p virtue-linux --all-targets -- -D warnings
cargo test -p virtue-core
cargo test -p virtue-linux
./linux/scripts/build-deb.sh
```

Notes:

- This is the most complete Rust CI workflow and is the baseline for Linux or shared-core changes.
- `build-deb.sh` is the packaging step CI runs after tests.

### Linux client integration test (`.github/workflows/client-linux.yml`, `client-linux-integration` job)

End-to-end smoke test: boots the api worker locally against a fresh D1 database (the api's own D1-backed `/hash` routes stand in for the standalone Rust hash-server in local dev, same as `scripts/launch.sh`), builds the real `virtue-linux` binary, runs its daemon under Xvfb (screenshot capture works for real against a blank virtual display, so no mocking code is needed), logs in as the seeded dev account, and asserts hash/batch rows actually land in the database after a short delay.

Run it locally from the repo root:

```bash
bun client/linux/scripts/integration-test.ts
```

Requires `bun`, `cargo`, `curl`, `xvfb-run` on `PATH` (on Debian/Ubuntu: `apt-get install xvfb`).

Notes:

- Written in TypeScript, run directly with `bun` -- no build step. Shares `client/scripts/integration-test-lib.ts` (port picking, api dev server bootstrap, dev-user seeding, D1 verification/retry) with the macOS integration test below.
- Runs against an isolated `HOME`/`XDG_CONFIG_HOME`/`XDG_STATE_HOME` in a temp directory, so it won't touch a real local `virtue` install.
- `client/linux/scripts/ci-login.ts` drives `virtue login`'s password prompt non-interactively via `script(1)` (Bun has no built-in pty allocation); it exports `ciLogin()` so the script can call it in-process instead of shelling out.
- Currently Linux-only. The same api-dev-server + seed + verify shape generalizes to other platforms; extending it is tracked as follow-up work per platform.

### macOS client CI (`.github/workflows/client-macos.yml`)

Run these on macOS.

From `client/`:

```bash
cargo build -p virtue-core
cargo build -p virtue-mac
cargo clippy -p virtue-core --all-targets -- -D warnings
cargo clippy -p virtue-mac --all-targets -- -D warnings
cargo test -p virtue-core
cargo test -p virtue-core --features testing --test scenarios
./mac/scripts/build-dmg.sh
```

Notes:

- `build-dmg.sh` validates the app bundle and DMG packaging path.

### Windows client CI (`.github/workflows/client-windows.yml`)

Run these on Windows.

From `client/`:

```powershell
cargo build --target x86_64-pc-windows-msvc -p virtue-core
cargo build --target x86_64-pc-windows-msvc -p virtue-windows
cargo clippy --target x86_64-pc-windows-msvc -p virtue-core --all-targets -- -D warnings
cargo clippy --target x86_64-pc-windows-msvc -p virtue-windows --all-targets -- -D warnings
cargo test --target x86_64-pc-windows-msvc -p virtue-core
cargo test --target x86_64-pc-windows-msvc -p virtue-core --features testing --test scenarios
cargo test --target x86_64-pc-windows-msvc -p virtue-windows
./windows/scripts/build-installer.ps1 -Profile Debug
```

Notes:

- CI installs NSIS before building the installer.
- Release builds use `-Profile Release -Version <build_label>`, but debug packaging is the PR-time smoke test.

### Android client CI (`.github/workflows/client-android.yml`)

Run these on a machine with Java 17, Android SDK, NDK `26.1.10909125`, and Rust Android targets installed.

From `client/android/`:

```bash
./gradlew --no-daemon :app:lintDebug :app:assembleDebug
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

## Deployment workflows

`deploy.yml` is not a PR validation workflow. It runs on pushes to `main` and `staging` and deploys `web`, `api`, `landing`, and `api-donate`.

If you need to mirror deployment locally:

- `api/`: `bun run deploy:staging` or `bun run deploy:prod`
- `web/`: `bun run deploy:staging` or `bun run deploy:prod`
- `landing/`: `bun run deploy:staging` or `bun run deploy:prod`
- `api-donate/`: `bun run deploy:staging` or `bun run deploy:prod`

These require the appropriate Cloudflare credentials and, for landing, GitHub release access.

## Minimum recommended local validation

If you want a practical rule instead of the full matrix:

- `api/` changes: run API typecheck, tests, and Prettier check.
- `web/` changes: run web typecheck, Prettier check, and build.
- `landing/` changes: run landing typecheck, Prettier check, and build.
- `shared-web/` changes: run shared-web Prettier check plus both `web` and `landing` checks.
- `client/core/` changes: run Linux Rust checks at minimum; add macOS/Windows/mobile checks when relevant to the code touched.
- `client/linux/` changes: run the Linux client CI commands.
- `client/mac/` changes: run the macOS client CI commands.
- `client/windows/` changes: run the Windows client CI commands.
- `client/android/` changes: run the Android debug lint/build command.
- `client/ios/` changes: run the iOS Rust build plus simulator build.
