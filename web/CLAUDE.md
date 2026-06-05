# CLAUDE.md — Web App

Preact + TypeScript SPA, built with Vite. Entry point: `src/index.tsx`.

## Cross-component contract files

These two files implement the TypeScript side of contracts shared with the Rust client. **Read `../CLAUDE.md` before editing them.**

- `src/utils/api/crypto.ts` — AES-256-GCM decrypt, HPKE unwrap, argon2id password derivation, hash-chain verification
- `src/utils/api/batch-materializer.ts` — fetches and decrypts the batch blob produced by the Rust client; decodes the msgpack event array

Changes to the encryption format, nonce layout, msgpack schema, or hash algorithm must mirror the Rust implementation in `client/core/src/crypto.rs` and `client/core/src/batch.rs`.

## Code map

### Entry & routing

| File            | What's here                                                                                                                                       |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/index.tsx` | `App` root, `AppShell` (auth-gate + router), `APIProvider` + `ToastProvider` wrappers, `GlobalEmailActionHandler` (sessionStorage flash messages) |
| `src/style.css` | Global CSS variables and base styles                                                                                                              |

### API / data layer (`src/utils/api/`)

| File                    | What's here                                                                                                                                                                                                              |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `api.ts`                | Raw `req()` fetch wrapper (auto-reauth on 401), `api` object with every typed endpoint call, re-exports all shared types from `@virtueinitiative/shared-web/types`                                                       |
| `session.ts`            | `Session` class — `fromLogin`, `fromFinishSignup`, `restore`; wrapping-key localStorage persistence; token-refresh handler                                                                                               |
| `client.ts`             | `APIClient` — observable caches for user / partners / devices; `queryLogs` entry point with concurrent batch-decryption workers                                                                                          |
| `hooks.tsx`             | `APIProvider`, `useAPIContext`, `useSetAPIClient`, `useUser`, `usePartners`, `useDevices`                                                                                                                                |
| `index.ts`              | Public re-exports: `login`, `requestSignup`, `finishSignup` convenience wrappers + everything from hooks                                                                                                                 |
| `crypto.ts`             | `derivePasswordMaterial` (argon2id+HKDF), `encryptData`/`decryptBatch` (AES-256-GCM), `generateUserKeyPair`/`importUserPrivateKey`/`unwrapBatchKey`/`encryptForPublicKey` (HPKE X25519), `decompressGzip`, `verifyBatch` |
| `batch-materializer.ts` | `decryptAndFlattenBatch` — fetch URL → unwrap key → AES-GCM decrypt → gzip decompress → msgpack decode → `verifyBatch` → return `FeedLog[]`                                                                              |
| `data-cache.ts`         | Dexie IndexedDB schema (`feeds`, `decryptedEvents`, `eventImages`); `mergeDataPageIntoCache`, `writeMaterializedEvents`, `queryDecryptedEvents`, `loadEventImage`, `pruneDecryptedEventsBefore`                          |

### Pages (`src/pages/`)

| File                       | What's here                                                                                                                                                   |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Auth/index.tsx`           | Login / signup / forgot-password / reset / finish-signup flows; `buildResetKeyMaterial`                                                                       |
| `Home/index.tsx`           | Dashboard: `DeviceCard`, `PartnerCard`, `PendingPartnerCard`, `InviteButton`, `AddDeviceButton`                                                               |
| `Logs/index.tsx`           | Log viewer shell: sidebar (device/user selector), date/risk/type filters via `useUrlState`, orchestrates `LogsList` / `LogsGallery`                           |
| `Logs/LogsList.tsx`        | Virtualised list (`@tanstack/react-virtual`), fixed row height 68 px                                                                                          |
| `Logs/LogsGallery.tsx`     | Justified image gallery with `buildGalleryRows` layout, virtualised rows                                                                                      |
| `Logs/shared.tsx`          | `FeedLog` type, `getLogMessage`, `getLogCategory`, `LOG_TYPES`, `EventImage` (async IDB image loader), `LogDetailDialog`, `formatDayLabel`/`formatDayAndTime` |
| `Logs/gallery-layout.ts`   | Pure `buildGalleryRows` — fits images into rows by aspect ratio given a container width                                                                       |
| `Settings/index.tsx`       | Profile/email/delete-account forms                                                                                                                            |
| `InviteAccept/index.tsx`   | One-shot partner-invite acceptance from a URL token                                                                                                           |
| `VerifyEmail/index.tsx`    | One-shot email verification from a URL token                                                                                                                  |
| `Dev/Components/index.tsx` | Component showcase (dev-only, dynamic import)                                                                                                                 |
| `_404.tsx`                 | 404 page                                                                                                                                                      |

### Shared utilities

| File                           | What's here                                                                                   |
| ------------------------------ | --------------------------------------------------------------------------------------------- |
| `src/utils/time.ts`            | `formatRelativeTimestamp`, `formatDate`, `formatTime`, `formatDayHeading`, `localDateKey`     |
| `src/utils/toast.ts`           | Module-level `sendToast` helper (stores the Preact `push` ref so crypto code can fire toasts) |
| `src/utils/webp-dimensions.ts` | `decodeWebpDimensions` — reads width/height from VP8/VP8L/VP8X binary headers                 |
| `src/hooks/useUrlState.ts`     | `useUrlState<T>` — syncs a value to a URL search param; supports string/number/boolean/object |
| `src/hooks/usePromise.ts`      | `usePromise` — `[pending, setPromise]` tuple for tracking async button state                  |
| `src/components/Header.tsx`    | App nav header with desktop + mobile drawer                                                   |

### Types

`src/utils/api/api.ts` re-exports all shared API types (`User`, `Device`, `Batch`, `DataLog`, etc.) from `@virtueinitiative/shared-web/types`. These mirror API response shapes — if the API changes, update here too.

Known log event `type` values are in `src/pages/Logs/shared.tsx` as `LOG_TYPES`.

### Testing

| File                    | What's here                                                  |
| ----------------------- | ------------------------------------------------------------ |
| `src/test-setup.ts`     | fake-indexeddb, MSW server start/stop, jest-dom matchers     |
| `src/test-utils.tsx`    | `makeFakeSession`, `mockSessionRestore`, `renderWithClient`  |
| `src/mocks/handlers.ts` | MSW request handlers for all API endpoints                   |
| `src/mocks/fixtures.ts` | `TEST_USER`, `TEST_DEVICES`, `TEST_WATCHER`, `TEST_WATCHING` |
