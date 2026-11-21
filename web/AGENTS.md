# AGENTS.md — Web App

Preact + TypeScript SPA, built with Vite. Entry point: `src/index.tsx`. What users see is
specified in `SPEC.md` (`WEB-` IDs); [README.md](README.md) covers running it.

## Cross-component contract files

These two files implement the TypeScript side of contracts shared with the Rust client. **Read `../docs/contracts.md` before editing them.**

- `src/utils/api/crypto.ts` — AES-256-GCM decrypt, HPKE unwrap, argon2id password derivation, hash-chain verification
- `src/utils/api/batch-materializer.ts` — fetches and decrypts the batch blob produced by the Rust client; decodes the msgpack event array

Changes to the encryption format, nonce layout, msgpack schema, or hash algorithm must mirror the Rust implementation in `client/core/src/crypto.rs` and `client/core/src/module/upload/batch.rs`.

## Where things are

- `src/utils/api/`: the API client, session and crypto. Every endpoint call goes through `api.ts`.
- `src/utils/cache/`: the OPFS-backed SQLite cache. A worker (`worker.ts`) owns the database; `client.ts` elects one tab as leader and fans results out to the others. [`architecture.md`](architecture.md) explains how it fits together.
- `src/pages/<Page>/index.tsx`: one folder per route.

## Rules

- **API types come from `shared-web/types.ts`.** `src/utils/api/api.ts` re-exports them. When an API response changes, update `shared-web/types.ts` first (see `api/AGENTS.md`).
- **Bump `CACHE_SCHEMA_VERSION`** in `src/utils/cache/schema.ts` when you change a cache table. A version or column mismatch rebuilds the cache instead of failing every query, but only if the declared schema matches what the code queries.
- **New log event types** need a `LogCaseKey` and a `LOG_KIND_TABLE` entry in `src/pages/Logs/shared.tsx`, or the Logs page shows them as unknown. Keep the retired lifecycle types there too (`unexpected_gap` and friends): old batches still contain them.
- **Pure logic goes in its own `.ts` file** next to the page (`src/pages/Report/report-model.ts`, `src/pages/Logs/gallery-layout.ts`), so it can be tested without rendering.

## Testing

Tests use Vitest, Testing Library and MSW.

- `src/mocks/handlers.ts` mocks every API endpoint; `src/mocks/fixtures.ts` holds the test user, devices and partners.
- `src/test-utils.tsx` has `renderWithClient` and fake sessions.
- Test what the user sees (WEB- behavior), not component internals. See `TESTING.md`.
