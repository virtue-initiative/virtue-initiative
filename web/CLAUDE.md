# CLAUDE.md — Web App

Preact + TypeScript app, built with Vite. Entry: `src/main.tsx`.

## Cross-component contract files

These two files implement the TypeScript side of contracts shared with the Rust client. **Read `../CLAUDE.md` before editing them.**

- `src/crypto.ts` — AES-256-GCM decrypt, HPKE unwrap, argon2id password derivation, hash chain
- `src/batch-materializer.ts` — decrypts and decodes the batch blob produced by the Rust client

Changes to the encryption format, nonce layout, msgpack schema, or hash algorithm must mirror the Rust implementation in `client/core/src/crypto.rs` and `client/core/src/batch.rs`.

## Data fetching

`src/swr.ts` wraps SWR for data fetching. All API calls go through `src/api.ts`.

## Types

`src/api.ts` exports all shared API types (`User`, `Device`, `Batch`, `DataLog`, etc.) and the `api` object with typed fetch wrappers. These types mirror the API response shapes — if the API changes, update `src/api.ts` to match.

Known log event `type` values are listed in `src/pages/Logs/shared.tsx` as `LOG_TYPES`.
