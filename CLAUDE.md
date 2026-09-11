# CLAUDE.md

## SPEC.md files

Some components have a SPEC.md file. These files MUST be written with RFC-like language. If you need to change one, you SHOULD keep your changes as minimal as possible.

SPEC.md is the source of truth and MUST be updated before the code is updated. They SHOULD NOT include full implementation details, but include enough to recreate something similar to the existing component.

Each numbered section is tagged with a stable ID scoped to its file (e.g. `API-032`, `HASH-005`, `CORE-002`), not a positional number, so cross-references in code comments survive reordering or insertion. A new section MUST get the next unused number for its file; numbers MUST NOT be reused, even after the section they named is deleted. IDs need not stay in numeric or document order. Code comments SHOULD cite the bare ID (e.g. `HASH-005`) rather than repeating the file path:

| Prefix | File                |
| ------ | ------------------- |
| `API`  | `api/SPEC.md`       |
| `HASH` | `hash-server/SPEC.md` |
| `CORE` | `client/core/SPEC.md` |

## Repo map

- `api/` — Cloudflare Workers REST API (TypeScript, Hono, D1 SQLite, R2 object storage)
- `api-donate/` — Standalone Cloudflare Worker for donations (TypeScript, Hono, D1, Stripe Checkout)
- `web/` — Main web app (TypeScript, Preact, Vite)
- `landing/` — Marketing/help site (TypeScript, Astro, Preact)
- `shared-web/` — Shared UI components used by `web/` and `landing/`
- `client/core/` — Shared Rust monitoring core (screenshot capture, encryption, batching, upload)
- `client/linux|mac|windows|android|ios/` — Platform-specific client wrappers

See `AGENTS.md` for how to run checks and tests for each component.

## Local dev

`./scripts/setup.sh` — one-time: installs deps, copies `.dev.vars`, runs local D1 migrations, installs/trusts Caddy.

`./scripts/launch.sh [--donate] [domain]` — starts `api`, `web`, `landing`, and the standalone `hash-server` (see `hash-server/SPEC.md`) together (interleaved colored logs), each on a random free port. The hash server is minted a `JWT_PUBLIC_KEY` read from `api/.dev.vars` so it verifies tokens signed by the local API:

- No `domain` arg: plain `http://localhost:<port>` for each service (ports differ per run — see the script's own startup banner). This is enough for most manual testing; browsers treat `http://localhost` as a secure context, so the API's `Secure` session cookie still gets set and sent.
- With a `domain` arg (e.g. `./scripts/launch.sh myfeature`): registers `https://app.<domain>.localhost`, `https://<domain>.localhost`, etc. via the local Caddy instance (requires `setup.sh` to have run), mimicking the production URL structure.
- `--donate` also starts `api-donate` and forwards Stripe webhooks to it if the `stripe` CLI is available.

`EMAIL_DELIVERY_MODE=log` in `api/.dev.vars` means outgoing emails aren't sent — they're printed to the `[api]`-prefixed dev-server log (subject/text/html/metadata), including the token-bearing links. That's the way to find a signup/email-change/password-reset link during local testing.

## Cross-component contracts

These five things are implemented independently in both Rust (`client/core/`) and TypeScript (`web/`) and **must stay bit-for-bit compatible**. If you change one side, you must change the other to match.

### 1. Batch wire format

Rust produces, TypeScript consumes:

```
events → msgpack({events: [...]}) → gzip → AES-256-GCM(batchKey)
wire:  nonce[12 bytes] || ciphertext+tag
```

Key files:

- Rust: `client/core/src/batch.rs`, `client/core/src/crypto.rs`
- TypeScript: `web/src/batch-materializer.ts`, `web/src/crypto.ts`

### 2. Per-event content hash

Used to build the hash chain uploaded to `POST /hash`:

```
contentHash = sha256(ts_le64 || type_utf8 || sorted(key_utf8 || encoded_value))
```

Value encoding: strings → UTF-8, numbers → i64 LE, booleans → 0x00/0x01, bytes → raw.

Key files:

- Rust: `client/core/src/crypto.rs`
- TypeScript: `web/src/crypto.ts` (`computeNewState`, `verifyBatch`)

### 3. Rolling hash state

```
new_state = sha256(current_state[32] || contentHash[32])
```

### 4. Password auth derivation

Login must hash the password the same way on both platforms before sending:

```
argon2id(password, salt=lowercase_email, m=65536, t=3, p=1, len=32)
then HKDF-SHA256("auth", argon_output) → hex string sent as password_auth
```

Key files:

- Rust: `client/core/src/api.rs`
- TypeScript: `web/src/crypto.ts` (`derivePasswordMaterial`)

### 5. HPKE key wrapping

Batch keys are wrapped per-recipient using `DhkemX25519HkdfSha256 / HkdfSha256 / Aes256Gcm`.
Wire format: `enc[kem.encSize bytes] || ciphertext`.

Key files:

- Rust: `client/core/src/crypto.rs`
- TypeScript: `web/src/crypto.ts` (`unwrapBatchKey`, `encryptForPublicKey`)

The `access_keys` JSON envelope and `DeviceSettings` shape are also shared between Rust and
the API, but are plain JSON relay shapes — not independently-reimplemented crypto — so they
aren't listed as one of the five contracts above. See `api/SPEC.md` for their wire shapes.

## Key invariant files

Read these before touching crypto, batch, or auth code:

- `client/core/architecture.md` — canonical design doc for the Rust core
- `api/SPEC.md` — full API endpoint specification

## What not to change without full cross-component review

- The AES-GCM nonce position or length (must be first 12 bytes)
- The msgpack schema (`{events: [...]}` at the outer level, each event double-encoded)
- The argon2id parameters or the HKDF label strings (`"auth"`, `"key"`)
- The JWT token `type` claim values (`"device"`, `"server"`)
- The hash chain input encoding rules (LE integers, sorted keys)

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
