# Virtue Initiative API Spec

**This document is written primarily by AI and may not be completely up to date.**

This document reflects the API currently implemented in `src/routes`.

Base URL examples:

- Local: `http://127.0.0.1:8787`
- Production: your deployed Workers URL

All timestamps are ISO-8601 UTC strings.

## Authentication model

- Access tokens are JWTs sent via `Authorization: Bearer <token>`.
- Refresh tokens are stored in an `httpOnly` cookie named `refresh_token`.
- Server defaults: access token TTL 15 min, refresh token TTL 365 days.

## Error format

```json
{ "error": "Unauthorized" }
```

Validation errors (Zod treeified):

```json
{
  "error": {
    "errors": [],
    "properties": { "email": { "errors": ["Invalid email"] } }
  }
}
```

---

## Health

### `GET /`

```json
{ "name": "BePure API", "version": "1.0.0", "status": "ok" }
```

---

## Auth

### `POST /signup`

```json
{ "email": "user@example.com", "password": "password123", "name": "Optional" }
```

Response `201`: `{ "user": { "id", "email", "created_at" }, "access_token" }`

### `POST /login`

```json
{ "email": "user@example.com", "password": "password123" }
```

Response `200`: `{ "user": { "id", "email" }, "access_token" }`
Sets `refresh_token` httpOnly cookie.

### `POST /logout`

Clears the refresh token cookie. Response `200`.

### `POST /token`

Refreshes the access token using the `refresh_token` cookie.
Response `200`: `{ "access_token" }`

---

## Devices

All device endpoints require `Authorization: Bearer <token>`.

### `POST /device`

```json
{ "name": "My Phone", "platform": "android" }
```

Response `201`: `{ "id": "uuid", "created_at": "..." }`

### `GET /device`

List devices. Optional query: `?user=<userId>` (requires accepted partnership).
Response `200`: `{ "devices": [...] }`

### `PATCH /device/:id`

```json
{ "name": "New Name", "enabled": true }
```

Response `200`: `{ "updated": true }`

### `DELETE /device/:id`

Response `200`: `{ "deleted": true }`

---

## Batches

Encrypted, compressed 1-hour data blobs stored in R2.
The blob content is AES-256-GCM encrypted and gzip-compressed client-side.
The server stores opaque bytes — decryption happens on the client using the user's E2EE key.

All batch endpoints require `Authorization: Bearer <token>`.

### `POST /batch`

Upload a 1-hour encrypted batch. Multipart form:

| Field              | Type        | Description                                   |
| ------------------ | ----------- | --------------------------------------------- |
| `file`             | binary      | Encrypted + compressed batch blob             |
| `device_id`        | string      | Device UUID                                   |
| `start_time`       | ISO-8601    | Start of the batch window                     |
| `end_time`         | ISO-8601    | End of the batch window                       |
| `start_chain_hash` | 64-char hex | SHA-256 of the first chain hash in this block |
| `end_chain_hash`   | 64-char hex | SHA-256 of the last chain hash in this block  |
| `item_count`       | integer     | Number of log+image items in the batch        |
| `size_bytes`       | integer     | Uncompressed plaintext size                   |

Response `201`:

```json
{
  "batch": {
    "id": "uuid",
    "r2_key": "user/{uid}/batches/{id}.enc",
    "start_time": "...",
    "end_time": "...",
    "created_at": "..."
  }
}
```

The encrypted blob is publicly accessible at the R2 bucket's public URL using `r2_key`.

### `GET /batch`

List batches. Query params: `device_id?`, `user?` (partner access), `cursor?`, `limit?` (max 100).

Response `200`:

```json
{
  "items": [{ "id", "device_id", "r2_key", "start_time", "end_time", "start_chain_hash", "end_chain_hash", "item_count", "size_bytes", "created_at" }],
  "next_cursor": "..."
}
```

### `GET /batch/:id`

Get metadata for a single batch.

Response `200`: `{ "batch": { ...all fields... } }`

---

## Hash Chain

For every buffered log, clients send the SHA-256 hash of the log in binary. This is then stored with the device like so.

```
current = sha256(current || content_hash)
```

When a batch is uploaded, the starting and current hash is stored with the
batch so that the web client can verify it once it decrypts it.

All hash endpoints require `Authorization: Bearer <token>`.

### `POST /hash`

Upload a content hash using binary

- **Content-Type**: `application/octet-stream`
- **Body**: `[device_id:16B][content_hash:32B]`

### `GET /hash`

Gets the latest hash

Query params:

| Param       | Required | Description |
| ----------- | -------- | ----------- |
| `device_id` | ✓        | Device UUID |

## Partners

### `POST /partner`

Invite a partner by email.

```json
{ "email": "partner@example.com", "permissions": { "view_data": true } }
```

Response `201`: `{ "partner": { "id", "partner_email", "status", "permissions", "created_at" } }`

### `GET /partner`

List all partnerships (owned + as partner). Response `200`: `{ "owned": [...], "asPartner": [...] }`

### `POST /partner/accept`

Accept a pending invite.

```json
{ "id": "<partner_record_id>" }
```

Response `200`: `{ "accepted": true }`

### `PATCH /partner/:id`

Update permissions (owner only).

```json
{ "permissions": { "view_data": true } }
```

Response `200`: `{ "updated": true }`

### `DELETE /partner/:id`

Remove a partnership. Either party can delete. Response `200`: `{ "deleted": true }`

---

## Settings

### `GET /settings`

Response `200`: `{ "settings": { "name", "timezone", "retention_days" } }`

### `PUT /settings`

```json
{ "name": "Alice", "timezone": "America/New_York", "retention_days": 90 }
```

Response `200`: `{ "updated": true }`
