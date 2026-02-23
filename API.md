# BePure API

This document reflects the API currently implemented in `api/src/routes`.

Base URL examples:

- Local: `http://127.0.0.1:8787`
- Production: your deployed Workers URL

All timestamps are ISO-8601 UTC strings.

## Authentication model

- Access tokens are JWTs sent via `Authorization: Bearer <token>`.
- Refresh tokens are stored in an `httpOnly` cookie named `refresh_token`.
- Current server defaults (`api/src/routes/auth.ts`):
  - Access token TTL: 1 hour
  - Refresh token TTL: 365 days

## Error format

There are two common shapes:

1. Simple string errors:

```json
{ "error": "Unauthorized" }
```

2. Zod validation errors (treeified):

```json
{
  "error": {
    "errors": [],
    "properties": {
      "email": { "errors": ["Invalid email address"] }
    }
  }
}
```

## Health

### `GET /`

Response `200`:

```json
{
  "name": "BePure API",
  "version": "1.0.0",
  "status": "ok"
}
```

## Auth endpoints

### `POST /signup`

Request JSON:

```json
{
  "email": "user@example.com",
  "password": "password123",
  "name": "Optional Name"
}
```

Response `201`:

```json
{
  "user": {
    "id": "uuid",
    "email": "user@example.com",
    "created_at": "2026-02-23T00:00:00.000Z"
  },
  "access_token": "jwt"
}
```

### `POST /login`

Request JSON:

```json
{
  "email": "user@example.com",
  "password": "password123"
}
```

Response `200`:

```json
{ "access_token": "jwt" }
```

### `POST /logout`

Clears refresh cookie.

Response `204` with empty body.

### `POST /token`

Reads `refresh_token` cookie and issues a new access token.

Response `201`:

```json
{ "access_token": "jwt" }
```

## Device endpoints

All require `Authorization: Bearer <access_token>`.

### `POST /device`

Request JSON:

```json
{
  "name": "Jeff-Mac",
  "platform": "macos",
  "avg_interval_seconds": 300
}
```

Response `201`:

```json
{
  "id": "uuid",
  "created_at": "2026-02-23T00:00:00.000Z"
}
```

### `GET /device`

Response `200`:

```json
[
  {
    "id": "uuid",
    "name": "Jeff-Mac",
    "platform": "macos",
    "last_seen_at": "2026-02-23T00:00:00.000Z",
    "last_upload_at": "2026-02-23T00:00:00.000Z",
    "interval_seconds": 300,
    "status": "online",
    "enabled": true
  }
]
```

### `PATCH /device/:id`

Request JSON (one or more fields):

```json
{
  "name": "Jeff-MacBook",
  "interval_seconds": 600,
  "enabled": true
}
```

Response `200`:

```json
{
  "id": "uuid",
  "updated": true
}
```

## Image endpoints

All require `Authorization: Bearer <access_token>`.

### `POST /image`

Uploads binary and creates metadata in one request.

Request: `multipart/form-data`

- `file` (required): image file part
- `device_id` (required): device UUID
- `sha256` (required): lowercase hex SHA-256 string
- `taken_at` (required): ISO datetime

Response `201`:

```json
{
  "image": {
    "id": "uuid",
    "status": "uploaded",
    "r2_key": "user/{userId}/images/{imageId}.webp",
    "taken_at": "2026-02-23T00:00:00.000Z",
    "created_at": "2026-02-23T00:00:00.000Z"
  }
}
```

Notes:

- There is no signed upload URL flow.
- The server currently stores the provided `sha256` metadata value; it does not currently recompute and verify content hash.

### `GET /image/:id`

Returns image bytes from R2 for an image owned by the authenticated user.

Response `200` with image body and `Content-Type` header.

## Log endpoints

All require `Authorization: Bearer <access_token>`.

### `POST /log`

Request JSON:

```json
{
  "type": "missed_capture",
  "device_id": "uuid",
  "image_id": "uuid",
  "metadata": {
    "reason": "capture_failed"
  }
}
```

`image_id` and `metadata` are optional.

Response `201`:

```json
{
  "id": "uuid",
  "created_at": "2026-02-23T00:00:00.000Z"
}
```

### `GET /log`

Optional query params:

- `device_id`
- `type`
- `cursor`
- `limit` (default 50, max 100)

Response `200`:

```json
{
  "items": [
    {
      "id": "uuid",
      "type": "missed_capture",
      "device_id": "uuid",
      "image_url": "http://127.0.0.1:8787/image/<image_id>",
      "metadata": {
        "reason": "capture_failed"
      },
      "created_at": "2026-02-23T00:00:00.000Z"
    }
  ],
  "next_cursor": "2026-02-23T00:00:00.000Z"
}
```

`next_cursor` is omitted when no more results remain.

## Partner endpoints

All require `Authorization: Bearer <access_token>`.

### `POST /partner`

Request JSON:

```json
{
  "email": "partner@example.com",
  "permissions": {
    "view_images": true,
    "view_logs": true
  }
}
```

Response `201`:

```json
{
  "id": "uuid",
  "status": "pending"
}
```

### `POST /partner/accept`

Request JSON:

```json
{ "id": "uuid" }
```

Response `200`:

```json
{ "id": "uuid" }
```

### `GET /partner`

Response `200`:

```json
[
  {
    "id": "uuid",
    "partner_email": "partner@example.com",
    "status": "accepted",
    "permissions": {
      "view_images": true,
      "view_logs": true
    },
    "role": "owner",
    "created_at": "2026-02-23T00:00:00.000Z"
  }
]
```

### `PATCH /partner/:id`

Request JSON:

```json
{
  "permissions": {
    "view_images": false
  }
}
```

Response `200`:

```json
{
  "id": "uuid",
  "permissions": {
    "view_images": false,
    "view_logs": true
  }
}
```

### `DELETE /partner/:id`

Response `204` with empty body.

## Settings endpoints

All require `Authorization: Bearer <access_token>`.

### `POST /settings`

Merges incoming fields with existing settings.

Request JSON:

```json
{
  "name": "Jeff",
  "timezone": "America/New_York",
  "retention_days": 30
}
```

Response `200`:

```json
{
  "name": "Jeff",
  "timezone": "America/New_York",
  "retention_days": 30
}
```

### `GET /settings`

Response `200` with stored settings, or defaults when absent:

```json
{
  "name": null,
  "timezone": "UTC",
  "retention_days": 30
}
```
