# Catalog of the commands devs actually run day to day, grouped by component.
# Run `just --list` to see everything. Each recipe is a thin delegation to an
# existing script or package.json command — see AGENTS.md for the exact
# command sequences CI runs.

set positional-arguments

# --- top-level ---------------------------------------------------------

# One-time repo bootstrap: installs deps, copies config, runs local migrations.
[group('dev')]
setup:
    ./scripts/setup.sh

# Start api, web, landing, and the standalone hash-server together.
[group('dev')]
dev domain="":
    ./scripts/launch.sh {{domain}}

# Same as `dev`, but also starts api-donate (and Stripe webhook forwarding).
[group('dev')]
dev-donate domain="":
    ./scripts/launch.sh --donate {{domain}}

# --- api -----------------------------------------------------------------

# Run the API worker locally (wrangler dev).
[group('api')]
[working-directory: 'api']
api-dev:
    bun run dev

# Run the API test suite (vitest run).
[group('api')]
[working-directory: 'api']
api-test:
    bun run test

# Typecheck the API worker.
[group('api')]
[working-directory: 'api']
api-typecheck:
    bun run typecheck

# Format the API worker with prettier.
[group('api')]
[working-directory: 'api']
api-format:
    bun run format

# --- api-donate ------------------------------------------------------------

# Run the donations worker locally (wrangler dev).
[group('api-donate')]
[working-directory: 'api-donate']
api-donate-dev:
    bun run dev

# Run the donations worker test suite.
[group('api-donate')]
[working-directory: 'api-donate']
api-donate-test:
    bun run test

# Typecheck the donations worker.
[group('api-donate')]
[working-directory: 'api-donate']
api-donate-typecheck:
    bun run typecheck

# Format the donations worker with prettier.
[group('api-donate')]
[working-directory: 'api-donate']
api-donate-format:
    bun run format

# --- web -------------------------------------------------------------------

# Run the web app dev server (vite).
[group('web')]
[working-directory: 'web']
web-dev:
    bun run dev

# Build the web app for production.
[group('web')]
[working-directory: 'web']
web-build:
    bun run build

# Run the web app test suite.
[group('web')]
[working-directory: 'web']
web-test:
    bun run test

# Typecheck the web app.
[group('web')]
[working-directory: 'web']
web-typecheck:
    bun run typecheck

# Format the web app with prettier.
[group('web')]
[working-directory: 'web']
web-format:
    bun run format

# --- landing -----------------------------------------------------------

# Run the landing site dev server (astro).
[group('landing')]
[working-directory: 'landing']
landing-dev:
    bun run dev

# Build the landing site for production.
[group('landing')]
[working-directory: 'landing']
landing-build:
    bun run build

# Typecheck the landing site (astro check).
[group('landing')]
[working-directory: 'landing']
landing-typecheck:
    bun run typecheck

# Format the landing site with prettier.
[group('landing')]
[working-directory: 'landing']
landing-format:
    bun run format

# --- shared-web --------------------------------------------------------

# Format shared-web with prettier.
[group('shared-web')]
[working-directory: 'shared-web']
shared-web-format:
    bun run format

# --- core (Rust) -------------------------------------------------------

# Run the shared Rust core's test suite.
[group('core')]
[working-directory: 'client']
core-test:
    cargo test -p virtue-core

# --- linux ---------------------------------------------------------------

# Build the Linux .deb package.
[group('linux')]
linux-build:
    ./client/linux/scripts/build-deb.sh

# Run the Linux client's test suite.
[group('linux')]
[working-directory: 'client']
linux-test:
    cargo test -p virtue-linux

# --- mac -------------------------------------------------------------------

# Build the macOS app bundle (macOS only). Pass args after the recipe name, e.g. `just mac-build --arch arm64 --profile debug`.
[group('mac')]
mac-build *args:
    ./client/mac/scripts/build-app.sh {{args}}

# --- windows -------------------------------------------------------------

# Build the Windows MSIX package (Windows only). Run `client/windows/scripts/build-msix.ps1 -?` for all params.
[group('windows')]
windows-build profile="Debug":
    pwsh ./client/windows/scripts/build-msix.ps1 -Profile {{profile}}

# Run the Windows client's test suite (Windows only).
[group('windows')]
[working-directory: 'client']
windows-test:
    cargo test --target x86_64-pc-windows-msvc -p virtue-windows

# --- android ---------------------------------------------------------------

# Build the app (+ Rust JNI core).
[group('android')]
android-build release="false":
    ./client/android/scripts/build.sh {{ if release == "true" { "--release" } else { "" } }}

# Build, install, and launch the app on the emulator.
[group('android')]
android-install release="false":
    ./client/android/scripts/install.sh {{ if release == "true" { "--release" } else { "" } }}

# Lint the debug build (closest thing Android has to a typecheck).
[group('android')]
[working-directory: 'client/android']
android-lint:
    ./gradlew --no-daemon :app:lintDebug

# Start the emulator and wait for boot.
[group('android')]
android-start headless="false":
    ./client/android/scripts/start-emulator.sh {{ if headless == "true" { "--headless" } else { "" } }}

# Stop the running emulator.
[group('android')]
android-stop:
    ./client/android/scripts/stop-emulator.sh

# Lock the emulator's screen.
[group('android')]
android-lock:
    ./client/android/scripts/lock-emulator.sh

# Show app logs.
[group('android')]
android-logs follow="false":
    ./client/android/scripts/logs.sh {{ if follow == "true" { "--follow" } else { "" } }}

# Force the emulator into Doze (deep idle).
[group('android')]
android-force-idle:
    ./client/android/scripts/force-idle.sh

# --- ios -------------------------------------------------------------------

# Build the iOS app (macOS + Xcode only). Run the script with --help for all options.
[group('ios')]
ios-build *args:
    ./client/ios/scripts/build-ios.sh {{args}}
