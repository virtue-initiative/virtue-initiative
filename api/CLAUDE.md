# CLAUDE.md — API

Cloudflare Workers API using Hono. Entry point: `src/index.ts`.

## Token types and TTLs

Two kinds of credential exist: **opaque session refresh tokens** (looked up in the `sessions` table) and **short-lived EdDSA JWTs**.

| Token                | Kind   | TTL        | Auth header             | Notes                                                                                                                                                 |
| -------------------- | ------ | ---------- | ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| web refresh token    | opaque | 1 year     | `refresh_token` cookie¹ | Authenticates user/web routes directly                                                                                                                |
| device refresh token | opaque | ~1000 yr   | `Bearer <token>`        | Authenticates `/d/*` routes directly                                                                                                                  |
| `device`             | JWT    | 1 hour     | `Bearer <jwt>`          | `POST /hash` on the standalone hash server (see `../hash-server/SPEC.md`), not this API; minted by `POST /d/device`, `GET /d/device`, `POST /d/batch` |
| `server`             | JWT    | 60 seconds | `Bearer <jwt>`          | `GET /hash` / `DELETE /hash` on the standalone hash server; minted by this API, `sub` ignored                                                         |

¹ The web refresh token is also accepted as `Bearer <token>` (used by the cache worker).

There is no access-token exchange: opaque refresh tokens authenticate their routes directly. Both JWT types authenticate against the standalone hash server, not any route on this API — this API only mints them. `sub` is the device ID for `device` tokens; ignored for `server` tokens.

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
- `src/lib/hash-server.ts` — client for the standalone hash server (`GET`/`DELETE /hash`), used by `device-only.ts`, `devices.ts`
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
