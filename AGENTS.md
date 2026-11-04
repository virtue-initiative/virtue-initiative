# AGENTS.md

Instructions for coding agents working in this repo. Humans: start with [`README.md`](README.md).

- [`docs/README.md`](docs/README.md) lists every doc and what it covers.
- [`docs/overview.md`](docs/overview.md) explains how the system fits together and what it is for. Read its goals and non-goals before reviewing a change: they say what matters here and what doesn't.
- Each component's `AGENTS.md`, where there is one, lists the rules that are easy to break there.

## SPEC.md files

Some components have a SPEC.md file. These files MUST be written with RFC-like language. If you need to change one, you SHOULD keep your changes as minimal as possible.

SPEC.md is the source of truth and MUST be updated before the code is updated. They SHOULD NOT include full implementation details, but include enough to recreate something similar to the existing component.

Each numbered section is tagged with a stable ID scoped to its file (e.g. `API-032`, `HASH-005`, `CORE-002`), not a positional number, so cross-references in code comments survive reordering or insertion. A new section MUST get the next unused number for its file; numbers MUST NOT be reused, even after the section they named is deleted. IDs need not stay in numeric or document order. Code comments SHOULD cite the bare ID (e.g. `HASH-005`) rather than repeating the file path:

| Prefix   | File                   |
| -------- | ---------------------- |
| `API`    | `api/SPEC.md`          |
| `DONATE` | `api-donate/SPEC.md`   |
| `WEB`    | `web/SPEC.md`          |
| `HASH`   | `hash-server/SPEC.md`  |
| `CORE`   | `client/core/SPEC.md`  |
| `CLIENT` | `client/SPEC.md`       |
| `BATCH`  | `batch-format/SPEC.md` |

`web/SPEC.md` and `client/SPEC.md` describe what users see and can do in the web app and the platform client apps. The others describe server, core and wire behavior.

## Tests

Read `TESTING.md` before writing or changing a test. In short:

- Every test MUST cite the spec ID it proves, in its title (TypeScript) or a comment above it (Rust, C#, Kotlin, Swift). `just spec-check` enforces this. A self-test of a test helper cites `NO-SPEC` instead.
- Test behavior through the component's public interface (HTTP, the `Scenario` DSL, what the user sees), so refactoring internals doesn't break tests.
- A behavior change updates the spec, then a test citing it, then the code. A bug fix adds a test that fails without the fix.
- When a test shows the code doesn't meet the spec and the fix is out of scope, mark the test `KNOWN-BUG` as `TESTING.md` describes instead of fixing or weakening it.

## Running checks

[`docs/checking-changes.md`](docs/checking-changes.md) says what to run and in what order. In short:

- While you work, run the narrowest tests first. `just tests-for <SPEC-ID>` lists the tests citing a spec section and prints a command that runs just those.
- Before you finish, run `just check`. It runs the CI checks for every area changed since `staging` (including uncommitted files), including `spec-check` and `doc-check`, and lists the platform checks it can't run on this OS. Say in your summary which of those you didn't run.
- For a change a user would see, also run it: the web app through `playwright-cli`, the Linux client through the `virtue-login` skill.
- Run Rust commands from `client/`, always naming the package (`-p virtue-core`).
- `just spec-coverage` prints spec coverage. [`docs/ci-checks.md`](docs/ci-checks.md) has the exact commands each CI workflow runs, including release packaging, which you need only when changing packaging.
- When a change makes a doc wrong, fix the doc in the same change. [`docs/README.md`](docs/README.md) lists which doc covers what.

## Local dev

[`docs/development.md`](docs/development.md) covers setup, `launch.sh`, seeding, the shared Rust build cache, dev config files and building the Apple apps from Linux. The things agents trip on:

- Start the stack with `./scripts/launch.sh` in the background, writing its output to a file. Never pipe it into `head`/`tail`, which wedges the API. Ports change every run; read them from its startup banner.
- The Workers deploy and run through Cloudflare's `cf` CLI, not Wrangler: every `cf` command inside a Worker needs `--mode`, and D1 commands take database IDs. See "Cloudflare CLI (`cf`)" in `docs/development.md`.
- `just seed` creates `dev@dev.com` / `devpassword` (admin; watches `sam@dev.com` and `jordan@dev.com`, same password) and a week of activity.
- `EMAIL_DELIVERY_MODE=log` in `api/.dev.vars` prints outgoing emails, including token-bearing links, to the `[api]` log instead of sending them.
- Check `web/` and `landing/` changes in a browser with `bunx playwright-cli` (the `playwright-cli` skill). See "Driving the web app in a browser" in `docs/development.md`.

## Cross-component contracts

These five things are implemented independently in both Rust (`client/core/`) and TypeScript (`web/`) and **must stay bit-for-bit compatible**. If you change one side, you must change the other to match.

The shared test vectors in `batch-format/vectors/` (BATCH-008) pin all five. Both sides' tests check them, so drift on either side fails CI.

### 1. Batch wire format

Rust produces, TypeScript consumes. `batch-format/SPEC.md` is the source of truth:

```
event → msgpack({ts, risk?, type, data?})
events → msgpack([event, ...]) → gzip → AES-256-GCM(batchKey)
wire:  nonce[12 bytes] || ciphertext+tag
```

Key files:

- Rust: `client/core/src/module/upload/batch.rs`, `client/core/src/crypto.rs`
- TypeScript: `web/src/utils/api/batch-materializer.ts`, `web/src/utils/api/crypto.ts`

### 2. Per-event content hash

Used to build the hash chain uploaded to `POST /hash`:

```
contentHash = sha256(msgpack-encoded event bytes, exactly as they appear in the batch)
```

See BATCH-006.

Key files:

- Rust: `client/core/src/crypto.rs` (`compute_event_hash`)
- TypeScript: `web/src/utils/api/crypto.ts` (`verifyBatch`)

### 3. Rolling hash state

```
new_state = sha256(current_state[32] || contentHash[32])
```

### 4. Password auth derivation

Login must hash the password the same way on both platforms before sending. The salt and argon2 parameters come from `GET /user/login-material` (API-015):

```
argon2id(password, salt=password_salt, m/t/p=params, len=32)
then HKDF-SHA256("auth", argon_output) → Base64 string sent as password_auth
```

Key files:

- Rust: `client/core/src/crypto.rs` (`derive_password_auth`)
- TypeScript: `web/src/utils/api/crypto.ts` (`derivePasswordMaterial`)

### 5. HPKE key wrapping

Batch keys are wrapped per-recipient using `DhkemX25519HkdfSha256 / HkdfSha256 / Aes256Gcm`.
Wire format: `enc[kem.encSize bytes] || ciphertext`.

Key files:

- Rust: `client/core/src/crypto.rs`
- TypeScript: `web/src/utils/api/crypto.ts` (`unwrapBatchKey`, `encryptForPublicKey`)

The `access_keys` JSON envelope and `DeviceSettings` shape are also shared between Rust and
the API, but are plain JSON relay shapes — not independently-reimplemented crypto — so they
aren't listed as one of the five contracts above. See `api/SPEC.md` for their wire shapes.

## Key invariant files

Read these before touching crypto, batch, or auth code:

- `client/core/architecture.md` — canonical design doc for the Rust core
- `api/SPEC.md` — full API endpoint specification

## What not to change without full cross-component review

- The AES-GCM nonce position or length (must be first 12 bytes)
- The msgpack schema (an array at the outer level, each event double-encoded)
- The argon2id parameters or the HKDF label strings (`"auth"`, `"key"`)
- The JWT token `type` claim values (`"device"`, `"server"`)
- The hash chain input (the encoded event bytes, hashed as-is)

## Copy style

Checklist for user-facing text (website, help docs, app UI copy):

- [ ] No em dashes — reasonable comma clause or a new sentence instead
- [ ] Every line ends with terminal punctuation
- [ ] No sentence opens on a dangling fragment before a colon
- [ ] No rhetorical questions used as scene-setting
- [ ] No "which of those..." style callbacks — say the answer plainly
- [ ] Long compound sentences split where each half stands alone
- [ ] No sentence that only restates what the next sentence already implies
- [ ] Instructions phrased as direct commands ("Update...", "Delete...")
- [ ] User-facing terms match what's in the UI, not internal jargon

## Pull requests

When creating a PR, follow the template at `.github/PULL_REQUEST_TEMPLATE.md`. Fill in every section: summary, changes (type of change + components touched), and testing.

For UI or copy changes, attach screenshots with `gh pr create --attach` (see `.claude/skills/playwright-cli/references/pr-attachments.md`).
