# Cross-component contracts

The Rust core (`client/core/`) writes batches and the web app (`web/`) reads them, with separate
implementations of the same formats in two languages. Both sides have to agree on five things to
the bit. This page lists those five, the files on each side, and how we keep them in step. If you
change one side, you **must** change the other to match in the same change.

The shared test vectors in `batch-format/vectors/` (BATCH-008) pin all five. Both sides' tests
check them, so drift on either side fails CI. If you change a contract on purpose, regenerate the
vectors too, and expect both test suites to need the new ones.

## 1. Batch wire format

Rust produces batches and TypeScript consumes them.
[`batch-format/SPEC.md`](../batch-format/SPEC.md) is the source of truth:

```
event → msgpack({ts, risk?, type, data?})
events → msgpack([event, ...]) → gzip → AES-256-GCM(batchKey)
wire:  nonce[12 bytes] || ciphertext+tag
```

- Rust: `client/core/src/module/upload/batch.rs`, `client/core/src/crypto.rs`
- TypeScript: `web/src/utils/api/batch-materializer.ts`, `web/src/utils/api/crypto.ts`

## 2. Per-event content hash

Each event's hash goes into the hash chain that the device posts to the hash server
(`POST /hash`) as it records:

```
contentHash = sha256(msgpack-encoded event bytes, exactly as they appear in the batch)
```

The web app hashes the event bytes exactly as it found them in the batch, before decoding them.
That way the two sides only have to agree on which bytes to hash, and either one can change how it
encodes or decodes an event without breaking verification (BATCH-006).

- Rust: `compute_event_hash` in `client/core/src/crypto.rs`
- TypeScript: `verifyBatch` in `web/src/utils/api/crypto.ts`

## 3. Rolling hash state

```
new_state = sha256(current_state[32] || contentHash[32])
```

The hash server keeps this running state for each device (HASH-006), and the API records it as the
batch's end hash when the batch is uploaded. The web app recomputes it over the batch's events and
marks every event in the batch unverified if the two disagree (WEB-016).

## 4. Password auth derivation

Login must hash the password the same way on both platforms before sending it. The salt and the
Argon2id parameters come from `GET /user/login-material` (API-015):

```
argon2id(password, salt=password_salt, m/t/p=params, len=32)
then HKDF-SHA256("auth", argon_output) → Base64 string sent as password_auth
```

The web app also derives `HKDF-SHA256("key", argon_output)` to encrypt the user's private key.
That one is web-only (see [`accounts-and-keys.md`](accounts-and-keys.md)).

- Rust: `derive_password_auth` in `client/core/src/crypto.rs`
- TypeScript: `derivePasswordMaterial` in `web/src/utils/api/crypto.ts`

## 5. HPKE key wrapping

Each batch key is wrapped once for each reader using
`DhkemX25519HkdfSha256 / HkdfSha256 / Aes256Gcm`. Wire format: `enc[kem.encSize bytes] ||
ciphertext`.

- Rust: `client/core/src/crypto.rs`
- TypeScript: `unwrapBatchKey` and `encryptForPublicKey` in `web/src/utils/api/crypto.ts`

## Shared JSON shapes

The `access_keys` JSON envelope and the `DeviceSettings` shape are also shared between Rust and the
API. They are plain JSON, so a mismatch shows up as a parse error that tests catch quickly, and
they are left off the list above for that reason. `api/SPEC.md` gives their shapes.

## What not to change without a review across components

- The AES-GCM nonce position or length (it must be the first 12 bytes)
- The msgpack schema (an array at the outer level, each event encoded separately)
- The Argon2id parameters or the HKDF labels (`"auth"`, `"key"`)
- The JWT `type` claim values (`"device"`, `"server"`)
- The hash chain input (the encoded event bytes, hashed as they are)

Changing any of these breaks every batch already uploaded, every client already installed, or
both. (The Argon2id parameters are the gentlest case, since `GET /user/login-material` hands them
out, but a change still has to keep working for every account created under the old ones.)
