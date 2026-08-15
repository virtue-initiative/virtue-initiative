# Hash API Server

## 1. Overview

The hash server SHOULD have one endpoint with three methods and one status endpoint.

### 1.1 Error Responses

All error responses (not **2xx**) MUST have this shape.

```json
{
  "code": "invalid_body",
  "message": "The request contains an invalid body",
  "details": "optional more details"
}
```

`code` is one of: `invalid_body`, `invalid_query`, `unauthorized`, `forbidden`, `sequence_conflict`, `internal_error`.

### 1.2 Authentication

JWTs MUST use `EdDSA` (Ed25519). The verification key is configured via the
`JWT_PUBLIC_KEY` environment variable (Ed25519 SPKI PEM), matching the main API's
`JWT_PUBLIC_KEY`/`JWT_PRIVATE_KEY` pair.

### 1.3 Backwards compat

The API MUST be prefixed with the current major version. For versions before `v1`, use `v0.x`. The server SHOULD return **HTTP 410 Gone** if it no longer supports a version.

```
  /v0.1/...
  /v0.2/...
  /v1/...
  /v2/...
```

## 2. Methods

### 2.1 `POST /hash`

The client MUST authenticate with this header.

```
Authorization: Bearer <JWT>`
```

The JWT SHOULD be signed by the main API server and MUST have 'device' as the type claim and the device ID as the sub.

The server MUST reject invalid JWTs with **HTTP 401**.

Client MUST send a 40 byte body, all integer fields little-endian.

```
[unix_time:u32][seq:u32][sha hash:32 bytes]
```

- `unix_time`: REQUIRED, ignored for now (not used for replay prevention).
- `seq`: REQUIRED, MUST be strictly greater than the last sequence number for the device, until it resets on DELETE. A device the server has never seen is treated identically to a freshly-reset device (last sequence number `0`), so `seq: 0` is never valid — the first accepted value for any device is `1` or higher.
- `sha hash`: REQUIRED, hash to be combined with the currently stored hash (see below).

The server MUST respond with **HTTP 400** if the body is invalid.

The server MUST respond with **HTTP 409** if the sequence number is not strictly greater than the previous sequence number.

**Hash Storage**

The server MUST take the hash and combine it with the stored hash in this way

```
stored = sha256(stored || hash)
```

The server MUST return **HTTP 201** if the new state has been written to disk and MUST NOT return **HTTP 201** otherwise.

The client SHOULD retry the request if it receives and error, but SHOULD NOT retry if it receives any of the following errors: `400`, `401`, `409`.

### 2.2 `GET /hash?devices=[device_ids]`

The client (in this case, the main API server) MUST authenticate with this header.

```
Authorization: Bearer <JWT>
```

The JWT SHOULD be signed by the main API server and MUST have 'server' as the type claim. The sub claim SHOULD be ignored for `server` type tokens.

The server MUST respond with **HTTP 401**, if the JWT is invalid.

**device_ids** MUST be a comma seperate list of valid IDs.

A valid ID is a UUID (matching the main API's device ID format). The server SHOULD
reject malformed IDs or a malformed list with **HTTP 400**.

On a valid request, the server MUST return the following JSON shape with **HTTP 200**, with one entry per device ID.

The server MUST return a ZERO hash, a 0 count and a 0 for last_received if it does not know of the device ID.

```json
{
  "device_id": {
     "hash": "hash_hex",
     "seq": 40,
     "last_received": 1786674101
  },
  ...
}
```

### 2.3 `DELETE /hash?device=device1`

The client MUST authenticate with this header.

```
Authorization: Bearer <JWT>
```

The JWT SHOULD be signed by the main API server and MUST have 'server' as the type claim. The sub claim SHOULD be ignored for `server` type tokens.

The server MUST reject invalid JWTs with **HTTP 401**.

The server SHOULD return **HTTP 400** on a malformed `device_id`

The server MUST reset a device's hash to ZERO and also set the sequence number to zero.

It SHOULD NOT reset the last_received time.

On success, the server MUST return **HTTP 200** with the following shape. With the data, BEFORE it was reset.

```json
{
  "hash": "hash_hex",
  "seq": 40,
  "last_received": 1786674101
}
```

### 2.4 `GET /`

This endpoint MUST not require authentication, and MUST return **HTTP 200** unless the server is down.

The endpoint SHOULD return the following response shape.

The endpoint MAY return a status other than "ok" IF it has a way to detect degraded status. (We currently don't)

```json
{
  "name": "Virtue Initiative Hash API",
  "version": "1.0.0",
  "commit": "51e8a2690a19adcfdbd62494cc8b2b83f24c560b",
  "status": "ok"
}
```

## 3. Implementation

### 3.1 High level overview

The server...

- SHOULD be implemented in rust.
- SHOULD serve strictly HTTP.
- SHOULD be behind a cloudflare tunnel which handles HTTPS.
- SHOULD use SQLite as its backend database
- SHOULD be as fast as possible

### 3.2 Server

The server SHOULD use tokio as it's runtime, with axum for HTTP routing.

Configuration is read from environment variables (a `.env` file is loaded if present):

- `JWT_PUBLIC_KEY` required
- `HOST` default `0.0.0.0`
- `PORT` default `8788`
- `DATABASE_PATH` default `hash-server.sqlite`
- `WRITE_BATCH_WINDOW_MS` default `20`

### 3.3 Database

SQLite (via `rusqlite`, bundled) SHOULD be configured in WAL with synchronous = full.

Writes MUST all be on one thread and writes within a configurable time window SHOULD be batched as one transaction, with no maximum batch size. Implemented as a dedicated OS
thread owning the write connection: it blocks for the first queued write, then drains
anything else that arrives within `WRITE_BATCH_WINDOW_MS` into the same transaction
before committing. `POST`/`DELETE` handlers send a request to this thread and await a
response; they never touch SQLite directly.

Writes MUST be fully written to the database before a successful response is returned to the client. Handlers only respond after the writer thread's transaction commits.

`GET /hash` is served from SQLite directly, multiple readers are allowed in WAL mode, and after a commit, everything is fine.

### 3.4 Logging

The server SHOULD log every request at level debug.

The server SHOULD NOT log the body of every request.

The server SHOULD log every unexpected error (5xx codes).

## 4. Testing

### 4.1 Performance

We SHOULD have a script that uses h2load to test the number of valid requests per second over http.

Details: `scripts/bench.sh` (`--h1`, i.e. plain HTTP) has two modes: `read`, which repeatedly
calls `GET /hash?devices=<id>` (idempotent, so it measures real sustained throughput),
and `write`, which repeatedly `POST`s a fixed body to `/hash` — only the first request
in the run is a durable write, since h2load cannot vary the request body per call to
give each request a strictly-increasing `seq`; every request after that is a fast 409.
Treat the `write` number as the ceiling for the auth + parse + write-queue path, not
for sustained disk-durable writes. Tokens for both modes are minted with
`cargo run --example mint_token -- <sub> <device|server> <private_key_pem_path>`.

### 4.2 CI

The rust unit and integration tests SHOULD be run in github CI.

## 5. Deployment

### 5.1 Location

Both a STAGING (on push/merge to staging) and PRODUCTION (on push/merge to main) deployment SHOULD be deployed to our oracle cloud A1 VM at hash.virtueinitiative.org port 22.

### 5.2 Method

The built binary and systemd service SHOULD be copied over SSH to the oracle cloud VM from within CI. The staging binary SHOULD be named `staging-virtue-hash` and the production binary SHOULD be named `virtue-hash`

The SSH key SHOULD be stored as a GitHub secret.

The systemd service MUST be restarted after the deploy.

Each server MUST be configured with its own `DATABASE_PATH`, so staging and production
never share a database.

The production server SHOULD use the default `PORT` (8788). The staging server SHOULD be
configured with `PORT=8789`. Cloudflared (section 5.3) MUST route to the matching port.

The staging server SHOULD be configured with RUST_LOG=debug.

### 5.3 Cloudflared

Cloudflared MUST be configured on the oracle cloud VM and MUST route staging-hash.virtueinitiative.org to the staging server and hash.virtueinitiative.org to the production server.
