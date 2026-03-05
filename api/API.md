# Virtue Initiative API Spec

This document reflects the API currently implemented in `api/src/routes`.

Base URL examples:

- Local: `http://127.0.0.1:8787`
- Production: your deployed Workers URL

All timestamps are ISO-8601 UTC strings.

## Current Unimplemented Changes

- Moved device specific endpoints into their own namespace `/d/`

## Authentication model

- Access tokens are JWTs sent via `Authorization: Bearer <token>`.
- Refresh tokens are stored in an `httpOnly` cookie named `refresh_token`.
- Current server defaults: access token TTL 1 hour, refresh token TTL 365 days.

## Error format

Typical:

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

Unhandled server errors:

```json
{ "error": "Internal server error", "message": "..." }
```

---

## Health

### `GET /`

```json
{ "name": "Virtue Initiative API", "version": "1.0.0", "status": "ok" }
```

---

## Auth

### `POST /signup`

Request:

```json
{ "email": "user@example.com", "password": "password123", "name": "Optional" }
```

Response `201`:

```json
{
  "user": { "id": "uuid", "email": "user@example.com", "created_at": "..." },
  "access_token": "..."
}
```

Also sets `refresh_token` httpOnly cookie.

### `POST /login`

Request:

```json
{ "email": "user@example.com", "password": "<client-side hash>" }
```

Response `200`:

```json
{ "access_token": "..." }
```

Also sets `refresh_token` httpOnly cookie.

### `POST /logout`

Clears the `refresh_token` cookie.

Response: `204 No Content`.

### `POST /token`

Refreshes access token using `refresh_token` cookie.

Response `201`:

```json
{ "access_token": "..." }
```

### `POST /e2ee` (auth required)

Request:

```json
{ "encryptedE2EEKey": "<base64>" }
```

Response `200`:

```json
{ "encryptedE2EEKey": "<base64>" }
```

### `GET /e2ee` (auth required)

Response `200`:

```json
{ "encryptedE2EEKey": "<base64 or null>" }
```

### `GET /me` (auth required)

Response `200`:

```json
{ "id": "uuid", "email": "user@example.com", "name": "Optional or null" }
```

### `PATCH /me` (auth required)

Request:

```json
{ "name": "New Name" }
```

Response `200`:

```json
{ "ok": true }
```

---

## Devices

All device endpoints require `Authorization: Bearer <token>`.

### `GET /device`

List devices. Optional query: `?user=<userId>` (accepted partnership required).

Response `200`:

```json
[
  {
    "id": "uuid",
    "name": "My Laptop",
    "platform": "linux",
    "last_seen_at": "...",
    "last_upload_at": "...",
    "status": "online",
    "enabled": true
  }
]
```

### `PATCH /device/:id`

Request:

```json
{ "name": "New Name", "enabled": true }
```

Response `200`:

```json
{ "id": "uuid", "updated": true }
```

---

## Batches and Logs

Encrypted, compressed data blobs stored in R2.
The blob content is AES-256-GCM encrypted and gzip-compressed client-side.
Server stores opaque bytes; decryption happens client-side with the user's E2EE key.

All batch endpoints require `Authorization: Bearer <token>`.

### `GET /batch`

List batches. Query params: `device_id?`, `user?`, `cursor?`, `limit?` (max 100).

Response `200`:

```json
{
  "items": [
    {
      "id": "uuid",
      "device_id": "uuid",
      "batch_url": "https://...",
      "start_time": "...",
      "end_time": "...",
      "start_chain_hash": "64-char-hex",
      "end_chain_hash": "64-char-hex",
      "item_count": 12,
      "size_bytes": 12345,
      "created_at": "..."
    }
  ],
  "next_cursor": "..."
}
```

### `GET /logs` (auth required)

Query params: `device_id?`, `user?`, `cursor?`, `limit?` (max 100).

Response `200`:

```json
{
  "items": [
    {
      "id": "uuid",
      "device_id": "uuid",
      "kind": "missed_capture",
      "metadata": [["reason", "capture_failed"]],
      "created_at": "..."
    }
  ],
  "next_cursor": "..."
}
```

---

## Partners

### `POST /partner` (auth required)

Invite partner by email.

Request:

```json
{ "email": "partner@example.com", "permissions": { "view_data": true } }
```

Response `201`:

```json
{ "id": "uuid", "status": "pending" }
```

### `POST /partner/accept` (auth required)

Request:

```json
{ "id": "partner_record_id", "encryptedE2EEKey": "<optional base64>" }
```

Response `200`:

```json
{ "id": "partner_record_id" }
```

### `GET /partner` (auth required)

Response `200`:

```json
[
  {
    "id": "uuid",
    "partner_user_id": "uuid",
    "partner_email": "partner@example.com",
    "status": "accepted",
    "permissions": { "view_data": true },
    "role": "owner",
    "created_at": "...",
    "encryptedE2EEKey": "<base64 or null>"
  }
]
```

### `PUT /partner/:id` (auth required)

For partner-side encrypted key update.

Request:

```json
{ "encryptedE2EEKey": "<base64>" }
```

Response `200`:

```json
{ "id": "uuid" }
```

### `PATCH /partner/:id` (auth required)

Owner updates permissions.

Request:

```json
{ "permissions": { "view_data": true } }
```

Response `200`:

```json
{ "id": "uuid", "permissions": { "view_data": true } }
```

### `DELETE /partner/:id` (auth required)

Either side may delete.

Response: `204 No Content`.

---

## Settings

### `GET /settings` (auth required)

Response `200`:

```json
{ "name": null, "timezone": "UTC", "retention_days": 30 }
```

### `POST /settings` (auth required)

Create/merge settings.

Request:

```json
{ "name": "Alice", "timezone": "America/New_York", "retention_days": 90 }
```

Response `200`:

```json
{ "name": "Alice", "timezone": "America/New_York", "retention_days": 90 }
```

---

## Public R2 Pass-through (primarily for dev)

### `GET /r2/*`

Returns encrypted batch blob bytes from R2 (no auth required).

Response content type: `application/octet-stream`.

---

# Device API

This is a "separate" API that device clients use.

## Authentication

Authentication is done using a non-expiring JWT that contain the device id.

## Endpoints

### `POST /d/device`

This creates a new device. It requires a user access token for the account
(obtained with `/login`).

Request:

```json
{ "name": "My Laptop", "platform": "linux" }
```

Response `201`:

```json
{ "id": "uuid", "created_at": "...", "token": "jwt" }
```

### `GET /d/device`

Returns the device info/settings.

```
{
    "id": "uuid",
    "name": "My Laptop",
    "platform": "linux",
    "enabled": true,
    "encryptedE2EEKey": "base64",
    "hashServer": "https://hash-server.example.com",
}
```

### `POST /d/batch`

Upload an encrypted batch as multipart form.

| Field        | Type     | Description                       |
| ------------ | -------- | --------------------------------- |
| `file`       | binary   | Encrypted + compressed batch blob |
| `device_id`  | string   | Device UUID                       |
| `start_time` | ISO-8601 | Start of batch window             |
| `end_time`   | ISO-8601 | End of batch window               |
| `item_count` | integer  | Number of items in batch blob     |
| `size_bytes` | integer  | Uploaded encrypted payload size   |

`start_chain_hash` and `end_chain_hash` are derived server-side from stored device state.

Response `201`:

```json
{
  "batch": {
    "id": "uuid",
    "batch_url": "https://.../user/{uid}/batches/{id}.enc",
    "start_time": "...",
    "end_time": "...",
    "created_at": "..."
  },
  "new_state_hex": "64-char-hex"
}
```

### `POST /d/logs`

Adds an "alert log"

Request:

```json
{
  "created_at": "...",
  "kind": "missed_capture",
  "metadata": [["reason", "capture_failed"]]
}
```

Response `201`:

```json
{
  "log": {
    "id": "uuid",
    "device_id": "uuid",
    "kind": "missed_capture",
    "metadata": [["reason", "capture_failed"]],
    "created_at": "..."
  }
}
```

## Hash Server

For each batch item hash upload:

```
new_state = sha256(current_state || content_hash)
```

All hash endpoints require `Authorization: Bearer <device_jwt>`.

### `POST /hash`

Upload a content hash as binary.

- Content-Type: `application/octet-stream`
- Body: exactly 32 bytes = `[content_hash:32B]`

Response `200`:

```json
{ "ok": true }
```

### `GET /hash`

Get the latest rolling state.

Response `200`:

```json
{ "state_hex": "64-char-hex" }
```

