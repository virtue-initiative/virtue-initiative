Below is a structured REST design for a screenshot accountability system backed by **Cloudflare R2** (S3-compatible object storage).

Assumptions:

* R2 stores image binaries.
* Metadata (users, logs, devices, partners) stored in a relational DB.
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

Sets the request token as an http-only cookie

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

Images are uploaded via pre-signed URL to avoid proxying large files through app server.

## `POST /image`

Creates image metadata and returns R2 upload URL.

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
  "upload_url": "https://r2-presigned-url"
}
```

Client uploads directly to R2.

---

# Logs (accountability events)

Represents structured events (missed screenshot, manual override, review).

## `POST /log`

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

**Query**

```
?device_id=
?type=
?user=
?cursor=
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
      "created_at": "..."
    }
  ],
  "next_cursor": "..."
}
```

---

# Devices

Represents registered client agents.

## `POST /device`

Register device.

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
  "api_key": "device-secret",
  "created_at": "..."
}
```

---

## `GET /device`

List devices.

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
    "status": "online|offline"
  }
]
```

---

## `PATCH /device/:id`

Update configuration.

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

```json
{
  "email": "partner@example.com",
  "permissions": {
    "view_images": true,
    "view_logs": true
  }
}
```

Response:

```json
{
  "id": "uuid",
  "status": "pending"
}
```

## `POST /accept-partner`

Accepts invite

```json
{
  id: "uuid"
}
```

Response:

```json
{
  id: "uuid"
}

```

---

## `GET /partner`

List partners. Get partner relationship.

---

## `PATCH /partner/:id`

Update permissions.

```json
{
  "permissions": {
    "view_images": false
  }
}
```

---

## `DELETE /partner/:id`

Revoke partner access.

---

# Settings

User-level preferences.

## `POST /settings`

Create or replace settings.

```json
{
  "name": "Andrew",
  "timezone": "America/Chicago",
  "retention_days": 30,
}
```

---

## `GET /settings`

```json
{
  "name": "Andrew",
  "timezone": "America/Chicago",
  "retention_days": 30,
}
```

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

# Security Model

* JWT for user endpoints.
* Device API key scoped to `/image` + `/log`.
* All R2 uploads/downloads via short-lived signed URLs.
* Store SHA-256 and verify after upload.
* Immutable images (no PATCH/DELETE).

