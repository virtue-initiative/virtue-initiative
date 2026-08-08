# CLAUDE.md — API

Cloudflare Workers API using Hono. Entry point: `src/index.ts`.

## Token types and TTLs

Two kinds of credential exist: **opaque session refresh tokens** (looked up in the `sessions` table) and **short-lived EdDSA JWTs**.

| Token                | Kind   | TTL        | Auth header             | Notes                                                    |
| -------------------- | ------ | ---------- | ----------------------- | -------------------------------------------------------- |
| web refresh token    | opaque | 1 year     | `refresh_token` cookie¹ | Authenticates user/web routes directly                   |
| device refresh token | opaque | ~1000 yr   | `Bearer <token>`        | Authenticates `/d/*` routes directly                     |
| `hash-server`        | JWT    | 1 hour     | `Bearer <jwt>`          | Hash-server routes (`/hash`); minted by `POST /d/token`  |
| `server`             | JWT    | 60 seconds | `Bearer <jwt>`          | Server-to-server (e.g. `DELETE /hash`, `GET /hash/info`) |

¹ The web refresh token is also accepted as `Bearer <token>` (used by the cache worker).

There is no access-token exchange: opaque refresh tokens authenticate their routes directly. The JWT `sub` is a device ID for `hash-server`/`server` tokens; the `type` claim distinguishes them.

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
- `src/middleware/auth.ts` — JWT verification, sets `c.get('sub')`
- `src/middleware/validation.ts` — Zod validation wrapper
- `src/routes/device-only.ts` — batch upload and device log endpoints
- `src/routes/batches.ts` — batch retrieval with access control
- `src/routes/updates.ts` — combined `GET /updates` (user + devices + partners in one round trip)
- `src/lib/views.ts` — shared response-builders used by `/user`, `/device`, `/partner`, and `/updates`
- `API.md` — complete endpoint specification

## Type source of truth

Response shapes shared with the web are defined as Zod schemas in `shared-web/types.ts` and imported from there by both the API (`src/lib/email-domain.ts` re-exports `emailFrequencySchema` and friends) and the web. When changing an API response shape, update `shared-web/types.ts` first, then update the route handler to match.

## Bindings

`c.env.DB` is the D1 database. `c.env.BUCKET` is the R2 bucket. See `src/types/bindings.ts` for the full `Env` interface.
