# CLAUDE.md — API

Cloudflare Workers API using Hono. Entry point: `src/index.ts`.

## Token types and TTLs

Two kinds of credential exist: **opaque session refresh tokens** (looked up in the `sessions` table) and **short-lived EdDSA JWTs**.

| Token                | Kind   | TTL        | Auth header                             | Notes                                                                                                                                                                                                                                                                            |
| -------------------- | ------ | ---------- | --------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| web refresh token    | opaque | 1 year     | `refresh_token` cookie¹                 | Authenticates user/web routes directly                                                                                                                                                                                                                                           |
| device refresh token | opaque | ~1000 yr   | `Bearer <token>`                        | Authenticates `/d/*` routes directly                                                                                                                                                                                                                                             |
| `hash-server`        | JWT    | 1 hour     | `Bearer <jwt>`                          | **Local-hash-server mode only** (`isLocalHashServer(env)` true — `HASH_SERVER_URL` unset or ends in `/api`); minted by `POST /d/device`, `GET /d/device`, `POST /d/batch`; unsigned, no pubkey                                                                                   |
| `device-cert`        | JWT    | 24 hours   | `Bearer <jwt>` + `X-Signature*` headers | **Remote-hash-server mode only**; minted by `buildDeviceState` from a caller-supplied `X-Device-Pubkey` header on every call to `POST /d/device`/`GET /d/device`/`POST /d/batch` — never persisted, so omitting the header on any such call is a 400, not a "not enrolled" state |
| `server`             | JWT    | 60 seconds | `Bearer <jwt>`                          | Server-to-server (e.g. `DELETE /hash`, the merged `GET /hash` info shape)                                                                                                                                                                                                        |

¹ The web refresh token is also accepted as `Bearer <token>` (used by the cache worker).

There is no access-token exchange: opaque refresh tokens authenticate their routes directly. The JWT `sub` is a device ID for `hash-server`/`server`/`device-cert` tokens; the `type` claim distinguishes them. See the root `CLAUDE.md`'s "Device-cert request signing" contract for the `X-Signature*` header / Ed25519 signature scheme that `device-cert` tokens pair with on `POST /hash`.

## Validation pattern

Routes use `validateZ` from `src/middleware/validation.ts` with a Zod schema:

```ts
app.post('/route', validateZ('json', schema), async (c) => { ... })
```

On failure it returns `{ error: 'Invalid request data', details: z.treeifyError(...) }` with status 400.

## Error format

All error responses:

```json
{ "error": "Human-readable message", "details": { ... } }
```

HTTP status codes: 400 bad request, 401 unauthorized, 403 forbidden, 404 not found, 409 conflict, 500 server error.

## Key files

- `src/lib/db.ts` — all D1 database queries
- `src/lib/hash-server.ts` — hash-chain state access (local D1 or remote hash server), used by `hashes.ts`, `device-only.ts`, `devices.ts`
- `src/lib/credentials.ts` — `verifyUserCredentials`, shared by `POST /login` and `POST /d/device`
- `src/lib/app-url.ts` — `getAppUrl`, shared by every route that links back to the web app in an email
- `src/middleware/auth.ts` — JWT verification, sets `c.get('sub')`
- `src/middleware/validation.ts` — Zod validation wrapper
- `src/routes/device-only.ts` — device registration, settings, and batch upload endpoints
- `src/routes/data.ts` — data retrieval with access control
- `API.md` — complete endpoint specification

## Type source of truth

Response shapes shared with the web are defined as Zod schemas in `shared-web/types.ts` and imported from there by both the API (`src/lib/email-domain.ts` re-exports `emailFrequencySchema` and friends) and the web. When changing an API response shape, update `shared-web/types.ts` first, then update the route handler to match.

## Bindings

`c.env.DB` is the D1 database. `c.env.BUCKET` is the R2 bucket. See `src/types/bindings.ts` for the full `Env` interface.
