# CLAUDE.md — API

Cloudflare Workers API using Hono. Entry point: `src/index.ts`.

## Token types and TTLs

| Type | TTL | Auth header | Notes |
|------|-----|-------------|-------|
| `access` | 1 hour | `Bearer <jwt>` | User routes |
| `device-access` | 7 days | `Bearer <jwt>` | Device routes (`/d/*`, `/hash`) |
| `server` | 60 seconds | `Bearer <jwt>` | Server-to-server (e.g. `DELETE /hash`) |
| refresh | 1 year | HTTPOnly cookie | Exchanged at `POST /token` |

JWT subject (`sub`) is a user ID for `access` tokens and a device ID for `device-access`/`server` tokens. The `type` claim in the payload distinguishes them.

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
- `src/routes/data.ts` — data retrieval with access control
- `API.md` — complete endpoint specification

## Type source of truth

The TypeScript types in `api/src/routes/` and `api/src/lib/db.ts` are the canonical definitions. `web/src/api.ts` mirrors the shapes that the API actually returns — keep them in sync when changing API response shapes.

## Bindings

`c.env.DB` is the D1 database. `c.env.BUCKET` is the R2 bucket. See `src/types/bindings.ts` for the full `Env` interface.
