# Local development

How to set up a machine, run the stack and test against it. Per-component details are in each
component's README; the platform clients are covered in [`client/README.md`](../client/README.md).

## Setup and running the stack

`./scripts/setup.sh` — one-time: installs deps, copies `.dev.vars`, runs local D1 migrations, seeds sample data, installs/trusts Caddy.

`./scripts/cow-cache.sh` (`just cow-cache <cmd>`) — optional shared Rust build cache. `init` (one-time, sudo) mounts a loop-backed XFS image at `/mnt/virtue-cow`; `warm` builds `client` (including the Android JNI library, if the Android toolchain is installed) and `hash-server` from `origin/staging` there; `setup.sh` then makes each worktree's `client/target` and `hash-server/target` symlinks to instant reflinked copies of that base, so only the workspace's own crates rebuild. If the mount or base build is missing, `setup.sh` skips it and cargo uses plain `target/` dirs. Rerun `warm` after a toolchain upgrade, and `prune` to free the space of deleted worktrees. `warm-windows` and `prune-windows` do the same for the Windows VM, on a Dev Drive inside it (see `client/windows/VM_SETUP.md`, step 8); `remote-windows-build.sh` then gives each worktree its own build root there.

`just seed` (`bun scripts/seed/index.ts`) — writes sample accounts, devices, partnerships and a week of encrypted, hash-chain-verified activity straight into local D1 and R2, so the Logs and Report tabs have something to show without a real device. Log in as `dev@dev.com` / `devpassword` (admin; watches `sam@dev.com` and `jordan@dev.com`, same password). Re-run it to move the sample week up to today, then use "Clear cache" in the web app. `--users-only` seeds just the accounts. The scenario lives in `scripts/seed/scenario.ts`; the placeholder screenshots come from `scripts/seed/make-images.ts`.

`./scripts/launch.sh [--donate] [--host ADDRESS | domain]` — starts `api`, `web`, `landing`, and the standalone `hash-server` (see `hash-server/SPEC.md`) together (interleaved colored logs), each on a random free port. The hash server is minted a `JWT_PUBLIC_KEY` read from `api/.dev.vars` so it verifies tokens signed by the local API:

- No `domain` arg: plain `http://localhost:<port>` for each service (ports differ per run — see the script's own startup banner). This is enough for most manual testing; browsers treat `http://localhost` as a secure context, so the API's `Secure` session cookie still gets set and sent.
- With a `domain` arg (e.g. `./scripts/launch.sh myfeature`): registers `https://app.<domain>.localhost`, `https://<domain>.localhost`, etc. via the local Caddy instance (requires `setup.sh` to have run), mimicking the production URL structure.
- With `--host ADDRESS` (a VPN or LAN address of this machine): every service listens there, so phones, VMs and other machines can reach the stack. Build a client against the "Devices" URL in the banner (see "Pointing a build at a local API" in [`client/README.md`](../client/README.md)); clients do their networking in Rust, so plain http works on every platform. The web app proxies `/api` and `/r2` to the API, and needs https away from localhost for Web Crypto, so set `VIRTUE_DEV_TLS_CERT` and `VIRTUE_DEV_TLS_KEY` to a certificate for that address that the viewing browser trusts.
- `VIRTUE_API_PORT`, `VIRTUE_WEB_PORT`, `VIRTUE_LANDING_PORT`, `VIRTUE_DONATE_PORT` and `VIRTUE_HASH_PORT` fix the ports, so a client build's API URL keeps working across restarts.
- `--donate` also starts `api-donate` and forwards Stripe webhooks to it if the `stripe` CLI is available.

To see a `web/` or `landing/` change in a browser, use `bunx playwright-cli` (the `playwright-cli` skill). It runs headless. See "Driving the web app in a browser" below.

`EMAIL_DELIVERY_MODE=log` in `api/.dev.vars` means outgoing emails aren't sent — they're printed to the `[api]`-prefixed dev-server log (subject/text/html/metadata), including the token-bearing links. That's the way to find a signup/email-change/password-reset link during local testing.

## Centralized dev config: `~/.config/virtue-dev.env` and `.env`

The user typically works across several worktrees of this repo at once. Two plain shell files
(`KEY=value` lines) layer together as the source of dev-time secrets and machine/worktree config,
in priority order (later wins over earlier, and real process/CI env vars always win over both):

1. `~/.config/virtue-dev.env` (overridable via `VIRTUE_DEV_ENV`) — the machine-wide layer.
   Anything genuinely identical across every worktree on a given machine (most secrets, SDK
   paths) goes here so it isn't re-entered per worktree.
2. `.env` at the repo root (gitignored; see `.env.example`) — the per-worktree override layer,
   for anything that should vary between worktrees (or that you don't want machine-wide).

Both are sourced by:

- `scripts/launch.sh` — exports the merged values to the dev servers it starts, and writes the
  `api/.dev.vars` keys below into `api/.dev.vars.launch` (a per-run copy of `.dev.vars` that
  `cf dev --mode launch` reads instead), so they take effect live without editing `.dev.vars`.
- `scripts/setup.sh` — sources them and copies any of the `.dev.vars` keys below that are set
  onto the freshly copied `.dev.vars` files, so a new worktree (or one run without `launch.sh`,
  e.g. `bun run dev` directly) still picks them up.
- `client/android/scripts/env.sh` — sources them as an override layer for the Android dev
  scripts (`build.sh`, `install.sh`, etc.), beneath its own built-in defaults.
- `client/core/build.rs` — reads them (repo-root `.env` first) to fill in whichever of the
  `VIRTUE_DEFAULT_*` compile-time keys below aren't already set by a real env var, replacing
  the old `client/.env`-only mechanism.

Recognized keys:

| Key                                                                                                                           | Used by                                                        |
| ----------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| `JWT_PRIVATE_KEY`, `JWT_PUBLIC_KEY`                                                                                           | `api/.dev.vars`                                                |
| `APP_NAME`, `API_BASE_PATH`, `EMAIL_DELIVERY_MODE`, `BUG_REPORT_EMAIL`                                                        | `api/.dev.vars`                                                |
| `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION`                                                                    | `api/.dev.vars` (SES)                                          |
| `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET`                                                                                  | `api-donate/.dev.vars`, `scripts/launch.sh --donate`           |
| `PUBLIC_STRIPE_PORTAL_URL`                                                                                                    | `scripts/launch.sh --donate` (landing "Manage donations" link) |
| `VIRTUE_DEFAULT_API_URL`, `VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS`, `VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS`                    | `client/core/build.rs` (compile-time client defaults)          |
| `ANDROID_SDK_ROOT`, `ANDROID_HOME`, `ANDROID_NDK_ROOT`, `ANDROID_NDK_HOME`, `VIRTUE_AVD`, `VIRTUE_PACKAGE`, `VIRTUE_ACTIVITY` | `client/android/scripts/*.sh`                                  |

Note: `APP_URL`, `LANDING_URL`, `R2_URL`, and `HASH_SERVER_URL` are _not_ on this list — `launch.sh`
always computes them itself from the ports it picks that run, so overriding them here would have
no effect under `launch.sh` (they'd only apply if you ran `bun run dev` directly).

## Driving the web app in a browser

Agents check UI changes in `web/` and `landing/` with Playwright CLI (`@playwright/cli`, a root
devDependency). The `playwright-cli` skill in `.claude/skills/` documents the commands. Run it as
`bunx playwright-cli`, not a global install. It runs headless by default, so it works on machines
with no display.

On a new machine, run `bun install` at the repo root, then `bunx playwright-cli install` once. That
uses an installed Chrome or Edge if it finds one, otherwise it downloads Chromium and writes a
local `.playwright/cli.config.json`. Its output goes to `.playwright-cli/`, which is gitignored. <!-- doc-check: ignore -->

1. Start the stack with `./scripts/launch.sh` in the background, writing its output to a file.
   Never pipe it into `head`/`tail`, which wedges the API. Read the Web/Landing URLs from its
   startup banner, because the ports change every run.
2. Log in with the account `setup.sh` seeds: `dev@dev.com` / `devpassword`.
3. Run `bunx playwright-cli close` when done, so headless browsers aren't left running.

Gotchas:

- Use refs from the latest snapshot only. Actions such as `open`, `goto` and `click` save their
  automatic snapshot to a `.playwright-cli/page-*.yml` file, while an explicit `snapshot` prints
  it inline. Ref prefixes change after a navigation or reload (`e16` becomes `f1e16`), so refs
  read from an older `.yml` file stop working.
- On the first visit after `launch.sh` starts, Vite may reload the page while it optimizes
  dependencies, which invalidates refs. Take a fresh snapshot if a ref is "not found".
- `screenshot --filename` must be a path inside the repo. An absolute path elsewhere (such as
  `/tmp`) is silently not written. Save the file inside the repo, then move it.
- `playwright-cli open` blocks `file:` URLs. To screenshot local HTML (such as a rendered email),
  serve it over http (for example `python3 -m http.server`) and open that.
- Attach PR screenshots with `gh pr create --attach`, or `gh pr edit --attach` on an existing PR
  (add `--repo` if running outside the checkout). It needs `gh` 2.99 or later, so upgrade `gh`
  if it is older. Do not commit screenshots to the repo or push them to a separate branch.

## Building the macOS and iOS apps from Linux

`client/scripts/remote-apple-build.sh` compiles the Rust on Linux and has a Mac compile the Swift, link and sign over SSH. Use it to test on a device. It's much faster than building Rust on the Mac, and CI still builds what ships.

```bash
just mac-remote                # Virtue.app, ad-hoc signed, in /Users/Shared/virtue-builds/<worktree>/ on the Mac.
just ios-remote --run          # Build for the simulator, then install and launch it in the booted one.
just ios-remote --destination device --profile release
```

- Apple's SDK isn't licensed for use off Apple hardware, so Linux doesn't have it. `client/scripts/apple-cross/apple-cc` compiles the C dependencies with Linux's `clang` and hand-written declarations in `client/scripts/apple-cross/include/` for the libc functions they call. A new C dependency that calls something else fails at compile time: add its declaration there.
- The daemon binary links on the Mac, through `apple-cross/apple-link`.
- Linux needs `clang`, `rsync` and SSH access to the Mac. The Mac needs Xcode. The default host is `virtue-ios@$VD_MAC_HOST`; override it with `--host` or `VIRTUE_APPLE_BUILD_HOST`.
- If the Mac's simulator runtime doesn't match Xcode's SDK, asset catalogs fail with `No simulator runtime version ... available`. Fix that once per account by pairing them, for example `xcrun simctl runtime match set iphoneos18.5 22G86`.
- `--prune` deletes the Mac's build dirs for worktrees that no longer exist.

Windows still builds entirely in the VM, through `remote-windows-build.sh`. `tract-linalg` needs Microsoft's `ml64.exe` and `lib.exe` at build time, and once a worktree has built, Rust is only about 15 seconds of a 60-second build, so cross-compiling it wouldn't pay. What keeps it fast is a current base: run `just cow-cache warm-windows` when the VM's first build in a new worktree recompiles most of the crates.

`vd <device> deploy` (see the `test-devices` skill) calls `client/scripts/install-on-test-device.sh` to build the current worktree, install it on a test device and launch it.

## Deployment workflows

Pushes to `main` and `staging` deploy everything, from three workflows: `deploy.yml` (`web`, `api`, `api-donate`), `deploy-landing.yml` (`landing`) and `deploy-hash-server.yml` (the hash server, see `hash-server/README.md`). The client apps are released from their own `client-*.yml` workflows (see each platform's `RELEASING.md`). `api-donate` has no separate release: a push to `staging` deploys it to production too.

If you need to mirror deployment locally:

- `api/`: `bun run deploy:staging` or `bun run deploy:prod`
- `web/`: `bun run deploy:staging` or `bun run deploy:prod`
- `landing/`: `bun run deploy:staging` or `bun run deploy:prod`
- `api-donate/`: `bun run deploy:staging` or `bun run deploy:prod`

These require the appropriate Cloudflare credentials and, for landing, GitHub release access.

## Cloudflare CLI (`cf`)

The Workers use Cloudflare's `cf` CLI (pinned in each Worker's `package.json`; it needs Node 24.22 or later), not Wrangler's own commands:

- Each Worker's bindings, routes and crons live in its `cloudflare.config.ts`. `--mode prod` and `--mode staging` select the deployment; local dev and tests use `staging`, and `--mode launch` is staging under its own name for `scripts/launch.sh`. `cf` loads the config for every command run inside a Worker's directory, so even account commands there need `--mode`. `wrangler.config.ts` beside it holds the few settings only Wrangler reads (dev port, static assets directory).
- `cf` still runs Wrangler underneath to build, serve and deploy, so `wrangler` stays a dependency. Local D1 and R2 data live in each Worker's `.cloudflare/state` (cf's default; older checkouts kept it in `.wrangler/state`, so rerun `bun run db:migrate:local` and `just seed` once).
- Tools that only understand Wrangler's config (the Vitest pool, the seed script's platform proxy) get it through `scripts/cf-config.ts`, which converts `cloudflare.config.ts` into `<worker>/.cloudflare/wrangler.<mode>.json`. Scripts that need a D1 database ID or SQL access use the same module.
- `cf`'s D1 commands take the database ID, not the binding: run migrations with the `db:migrate:*` scripts (`scripts/d1-migrate.ts`), and query local data with `bun run cf d1 raw <id> --mode staging --local --sql "..."` from `api/` (there is no local `cf d1 query`).
- `cf deploy` builds `web/` and `landing/` with their framework (Vite, Astro) when it detects one, which needs a Cloudflare plugin these sites don't use. Their deploy scripts build `dist/` themselves, package it with `cf-wrangler build` (the Wrangler entry point `cf` itself calls) and ship that with `cf deploy --prebuilt`.
- `cf dev` passes only `--port` (and `--mode`, `--host`, `--local`) on to Wrangler. To make the API or donate Worker listen on another address, set `DEV_IP` (for example `DEV_IP=0.0.0.0 bun run dev`), which their `wrangler.config.ts` reads. `launch.sh --host` does this for you.
- `cf` can't stream logs or set a single secret yet. Use `bunx wrangler tail <worker>` and `bunx wrangler secret put <NAME> --name <worker>` for those.
- `cf` only finds Wrangler in a Worker's own `node_modules`, so `bunfig.toml` uses Bun's isolated linker. A package a workspace imports must be in that workspace's `package.json`, and root `scripts/` imports must be in the root one.
