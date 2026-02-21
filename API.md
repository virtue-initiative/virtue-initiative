Below is a structured REST design for a screenshot accountability system backed by **Cloudflare R2** (S3-compatible object storage).

Assumptions:

* R2 stores image binaries.
* Metadata (users, logs, devices, partners) stored in Cloudflare D1 (serverless SQL).
* Auth via JWT (short-lived access + refresh).
* All timestamps ISO-8601 UTC.
* Images immutable once uploaded.
* Passwords are hashed with argon2

---

# Authentication

## `POST /signup`

Create user.

**Request**

```json
{
  "email": "user@example.com",
  "password": "plaintext",
  "name": "Optional",
}
```

**Response 201**

Sets the refresh token as a http-only cookie.

```json
{
  "user": {
    "id": "uuid",
    "email": "user@example.com",
    "created_at": "2026-02-20T14:22:00Z"
  },
  "access_token": "jwt",
}
```

---

## `POST /login`

**Request**

```json
{
  "email": "user@example.com",
  "password": "..."
}
```

**Response 200**

Sets the refresh token as an http-only cookie

```json
{
  "access_token": "jwt",
}
```

---

## `POST /logout`

Deletes the refresh token cookie

**Response 204**

---

## `POST /token`

Reads refresh token from cookie

**Response 201**

```
{
  "access_token": "jwt"
}
```

---

# Images (R2-backed)

Images are uploaded via pre-signed URL directly to R2 — the Worker is not in the upload path.

## `POST /image`

Creates image metadata and returns a presigned R2 PUT URL.

Requires: `Authorization: Bearer <access_token>`

**Request**

```json
{
  "device_id": "uuid",
  "sha256": "hexhash",
  "content_type": "image/png",
  "size_bytes": 153221,
  "taken_at": "2026-02-20T14:21:55Z"
}
```

**Response 201**

```json
{
  "image": {
    "id": "uuid",
    "status": "pending_upload",
    "r2_key": "user/{userId}/images/{imageId}.png",
    "taken_at": "2026-02-20T14:21:55Z",
    "created_at": "2026-02-20T14:22:00Z"
  },
  "upload_url": "https://<bucket>.<account>.r2.cloudflarestorage.com/...?X-Amz-Signature=..."
}
```

Client uploads directly to R2 via `PUT <upload_url>` with the binary body and matching `Content-Type`.

---

# Logs (accountability events)

Represents structured events (missed screenshot, manual override, review).

## `POST /log`

Requires: `Authorization: Bearer <access_token>`

**Request**

```json
{
  "type": "missed_capture",
  "device_id": "uuid",
  "image_id": "uuid|null",
  "metadata": {
    "reason": "device_offline"
  }
}
```

**Response 201**

```json
{
  "id": "uuid",
  "created_at": "..."
}
```

---

## `GET /log`

Requires: `Authorization: Bearer <access_token>`

**Query**

```
?device_id=
?type=
?cursor=
?limit=
```

**Response**

```json
{
  "items": [
    {
      "id": "uuid",
      "type": "missed_capture",
      "device_id": "uuid",
      "image_url": "signed_url",
      "metadata": {},
      "created_at": "..."
    }
  ],
  "next_cursor": "..."
}
```

`image_url` is a presigned R2 GET URL (1 hour expiry). `next_cursor` is omitted when there are no more results.

---

# Devices

Represents registered client agents. All device endpoints use the same JWT authentication as user endpoints.

## `POST /device`

Register device.

Requires: `Authorization: Bearer <access_token>`

**Request**

```json
{
  "name": "Andrew-Laptop",
  "platform": "linux",
  "avg_interval_seconds": 300
}
```

**Response 201**

```json
{
  "id": "uuid",
  "created_at": "..."
}
```

---

## `GET /device`

List devices.

Requires: `Authorization: Bearer <access_token>`

Return device metadata + stats:

```json
[
  {
    "id": "uuid",
    "name": "Andrew-Laptop",
    "platform": "linux",
    "last_seen_at": "...",
    "last_upload_at": "...",
    "interval_seconds": 300,
    "status": "online|offline",
    "enabled": true
  }
]
```

---

## `PATCH /device/:id`

Update configuration.

Requires: `Authorization: Bearer <access_token>`

```json
{
  "name": "New Name",
  "interval_seconds": 600,
  "enabled": true
}
```

---

# Partners (Accountability partner users)

Represents another user who can review your logs/images.

## `POST /partner`

Send partner invite.

Requires: `Authorization: Bearer <access_token>`

```json
{
  "email": "partner@example.com",
  "permissions": {
    "view_images": true,
    "view_logs": true
  }
}
```

Response 201:

```json
{
  "id": "uuid",
  "status": "pending"
}
```

## `POST /partner/accept`

Accepts invite. The authenticated user must be the invited partner.

Requires: `Authorization: Bearer <access_token>`

```json
{
  "id": "uuid"
}
```

Response 200:

```json
{
  "id": "uuid"
}
```

---

## `GET /partner`

List all partnerships (both as owner and as partner).

Requires: `Authorization: Bearer <access_token>`

```json
[
  {
    "id": "uuid",
    "partner_email": "partner@example.com",
    "status": "pending|accepted",
    "permissions": { "view_images": true, "view_logs": true },
    "role": "owner|partner",
    "created_at": "..."
  }
]
```

---

## `PATCH /partner/:id`

Update permissions. Only the owner (invite sender) may update.

Requires: `Authorization: Bearer <access_token>`

```json
{
  "permissions": {
    "view_images": false
  }
}
```

Response 200:

```json
{
  "id": "uuid",
  "permissions": { "view_images": false, "view_logs": true }
}
```

---

## `DELETE /partner/:id`

Revoke partner access. Only the owner may delete.

Requires: `Authorization: Bearer <access_token>`

**Response 204**

---

# Settings

User-level preferences stored as a JSON object. `POST` merges with existing settings (partial update).

## `POST /settings`

Requires: `Authorization: Bearer <access_token>`

```json
{
  "name": "Andrew",
  "timezone": "America/Chicago",
  "retention_days": 30,
}
```

**Response 200** — returns the full merged settings object:

```json
{
  "name": "Andrew",
  "timezone": "America/Chicago",
  "retention_days": 30
}
```

---

## `GET /settings`

Requires: `Authorization: Bearer <access_token>`

```json
{
  "name": "Andrew",
  "timezone": "America/Chicago",
  "retention_days": 30
}
```

Defaults: `timezone = "UTC"`, `retention_days = 30`, `name = null`.

---

# R2 Object Layout

Recommended bucket structure:

```
bucket/
  user/{userId}/
    images/{imageId}.webp
```

Metadata stored in DB; R2 stores only binary.

---

# Validation Errors

All endpoints validate requests with Zod. Invalid requests return **400** with a structured error body:

```json
{
  "error": {
    "formErrors": [],
    "fieldErrors": {
      "email": ["Invalid email"],
      "password": ["String must contain at least 8 character(s)"]
    }
  }
}
```

---

# Security Model

* JWT for all endpoints (access token in `Authorization: Bearer` header).
* Refresh token stored in httpOnly, secure, sameSite=Strict cookie.
* All R2 uploads/downloads via short-lived presigned S3 URLs (5 min upload, 1 hr download).
* Store SHA-256 and verify after upload.
* Immutable images (no PATCH/DELETE on images).
* Passwords hashed with Argon2id.

