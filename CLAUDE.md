# CLAUDE.md

## Repo map

- `api/` — Cloudflare Workers REST API (TypeScript, Hono, D1 SQLite, R2 object storage)
- `web/` — Main web app (TypeScript, Preact, Vite)
- `landing/` — Marketing/help site (TypeScript, Astro, Preact)
- `shared-web/` — Shared UI components used by `web/` and `landing/`
- `client/core/` — Shared Rust monitoring core (screenshot capture, encryption, batching, upload)
- `client/linux|mac|windows|android|ios/` — Platform-specific client wrappers

See `AGENTS.md` for how to run checks and tests for each component.

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

## Key invariant files

Read these before touching crypto, batch, or auth code:

- `client/core/architecture.md` — canonical design doc for the Rust core
- `api/API.md` — full API endpoint specification

## What not to change without full cross-component review

- The AES-GCM nonce position or length (must be first 12 bytes)
- The msgpack schema (`{events: [...]}` at the outer level, each event double-encoded)
- The argon2id parameters or the HKDF label strings (`"auth"`, `"key"`)
- The JWT token `type` claim values (`"access"`, `"device-access"`, `"server"`)
- The hash chain input encoding rules (LE integers, sorted keys)
