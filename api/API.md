# Virtue Initiative API Spec

Base URL examples:

- Local: `http://127.0.0.1:8787`
- Production: your deployed Workers URL

# Types

## Basic Types

- `DateTime`: A ms-precision timestamp using the unix epoch.
- `Image`: A binary webp image blob.
- `RefreshToken`: A JWT used for refreshing the AccessToken. The `sub` is the user id.
- `AccessToken`: A short term JWT used for authenticating with the API. The `sub` is the user id.
- `DeviceToken`: A non-expiring JWT that a device obtains. The `sub` is the device id.
- `Argon2Hash`: A password pre-hashed with Argon2 using the user's email + the domain as a salt
- `SHA256`: A hex encoded SHA256 hash
- `UserEncryptedData(...)`: Data encrypted with a key derived from the user's id and plaintext password.
- `PartnerEncryptedData(...)`: Data encrypted with a partner's public key.
- `E2EEData(...)`: Data encrypted with the user's E2EE encryption key using AES-256-GCM
- `Compress(...)`: Data gzip compressed

## Error

```json
{
  "error": "Unauthorized",
  "details": [object]
}
```

Validation errors (Zod treeified):

```json
{
  "error": "Bad Request",
  "details": {
    "errors": [],
    "properties": { "email": { "errors": ["Invalid email"] } }
  }
}
```

Unhandled server errors:

```json
{
  "error": "Internal server error",
  "details": {
    "message": "..."
  }
}
```

## Log

Base type.

```json
{
  "ts": DateTime,
  "type": "string",
  "data": {
    "key": "value",
  }
}
```

The API must allow more fields to be added to `data`

### Image

```json
{
  "ts": DateTime,
  "type": "image",
  "data": {
    "image": ImageData,
  }
}
```

### System Event

```json
{
  "ts": DateTime,
  "type": "system_event",
  "data": {
    "event": "startup"
  }
}
```

## Batch

```json
{
  "events": [Log]
}
```

## BatchData

```json
{
  "id": "uuid",
  "start": DateTime,
  "end": DateTime,
  "start_hash": "SHA256",
  "end_hash": "SHA256",
  "url": "url hosting E2EEData(Compressed(Batch))",
}
```

# Main API

## Authentication model

- Access tokens are JWTs sent via `Authorization: Bearer <token>`.
- Refresh tokens are stored in an `httpOnly` cookie named `refresh_token`.
- Current server defaults: access token TTL 1 hour, refresh token TTL 365 days.

---

## Health

### `GET /`

```json
{
  "name": "Virtue Initiative API",
  "version": "1.0.0",
  "status": "ok"
}
```

---

## Auth

### `POST /signup`

Request:

```json
{
  "email": "user@example.com",
  "password": "Argon2Hash",
  "name": "optional string"
}
```

Response `201`:

```json
{
  "user": {
    "id": "uuid",
    "email": "user@example.com",
    "name": "string" or undefined,
  },
  "access_token": "AccessToken"
}
```

Also sets `refresh_token` httpOnly cookie.

### `POST /login`

Request:

```json
{
  "email": "user@example.com",
  "password": "Argon2Hash"
}
```

Response `200`:

```json
{ "access_token": "AccessToken"}
```

Also sets `refresh_token` httpOnly cookie.

### `POST /logout`

Clears the `refresh_token` cookie.

Response: `204 No Content`.

### `POST /token`

Refreshes access token using `refresh_token` cookie.

Response `201`:

```json
{ "access_token": "AccessToken" }
```

### `GET /user` (auth required)

Response `200`:

```json
{
  "id": "uuid",
  "email": "user@example.com",
  "name": "string" or undefined,
  "e2ee_key": UserEncryptedData("base64") or undefined,
}
```

### `PATCH /user` (auth required)

Request:

```json
{
  "name": "optional string",
  "e2ee_key": optional UserEncryptedData("base64"),
}
```

Response `200`:

```json
{ "ok": true }
```

---

## Devices

All device endpoints require `Authorization: Bearer <token>`.

### `GET /device`

List devices that a user has access to.

Response `200`:

```json
[
  {
    "id": "uuid",
    "owner": "uuid",
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

All batch endpoints require `Authorization: Bearer <token>`.

### `GET /data`

List batches and other logs. Query params: `device_id?`, `user?`, `cursor?`,
`limit?` (max 100).

Response `200`:

```json
{
  "batches": [
    {
      "device_id": "uuid",
      ...BatchData,
    }
  ],
  "logs": [
    {
      "device_id": "uuid",
      ...Log,
    }
  ],
  "next_cursor": DateTime
}
```

---

## Partners

### `GET /pubkey?email=[email]`

Gets the public key for a user (used to distribute the e2ee\_key)

### `POST /partner` (auth required)

Invite partner by email.

Request:

```json
{
  "email": "partner@example.com",
  "permissions": { "view_data": true },

  // If the account already exist
  "e2ee_key": "optional PartnerEncryptedData(binary)",
}
```

Response `201`:

```json
{ "id": "uuid", "status": "pending" }
```

### `POST /partner/accept` (auth required)

Request:

```json
{ "id": "partner_record_id" }
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
    "partner": {
      "id": "uuid",
      "email": "partner@example.com",
      "name": "string" or undefined,
    },
    "status": "accepted",
    "permissions": { "view_data": true },
    "created_at": DateTime,
    "e2ee_key": "optional PartnerEncryptedData(binary)"
  }
]
```

### `PATCH /partner/:id` (auth required)

Owner can update.

Request:

```json
{
  "permissions": { "view_data": true },
  "e2ee_key" "PartnerEncryptedData(binary)"
}
```

Response `200`:

```json
{ "id": "uuid", "permissions": { "view_data": true } }
```

### `DELETE /partner/:id` (auth required)

Either side may delete.

Response: `204 No Content`.

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
{ "id": "uuid", "created_at": DateTime, "token": "jwt" }
```

### `GET /d/device`

Returns the device info/settings.

```
{
    "id": "uuid",
    "name": "My Laptop",
    "platform": "linux",
    "enabled": true,
    "e2ee_key": "base64",
    "hash_base_url": "https://hash-server.example.com",
}
```

### `POST /d/batch`

Upload an encrypted batch as multipart form.

| Field        | Type     | Description                       |
| ------------ | -------- | --------------------------------- |
| `file`       | binary   | Encrypted + compressed batch blob |
| `start_time` | DateTime | Start of batch window             |
| `end_time`   | DateTime | End of batch window               |

Response `201`:

```json
{
  "batch": BatchData,
  "new_start_hash": "SHA256"
}
```

### `POST /d/logs`

Adds a non-batched log.

Request:

```json
{
  ...Log without "id",
}
```

Response `201`:

```json
{
  "log": Log
}
```

## Hash Server

For each batch item hash upload:

```
new_state = sha256(current_state || content_hash)
```

All hash endpoints require `Authorization: Bearer <DeviceToken>`.

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

Content-Type: `application/octet-stream`

Response `200`

```json
[hash:16B]
```

