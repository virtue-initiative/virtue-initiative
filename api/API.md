# Virtue Initiative API Spec

Base URL examples:

- Local: `http://127.0.0.1:8787`
- Production: your deployed Workers URL

# Types

## Basic Types

- `Base64`: Base64 encoded data
- `UUID`: A uuidv4
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

```js
{
  "error": "Unauthorized",
  "details": Object
}
```

Validation errors (Zod treeified):

```js
{
  "error": "Bad Request",
  "details": {
    "errors": [],
    "properties": { "email": { "errors": ["Invalid email"] } }
  }
}
```

Unhandled server errors:

```js
{
  "error": "Internal server error",
  "details": {
    "message": "..."
  }
}
```

## Log

Base type.

```js
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

```js
{
  "ts": DateTime,
  "type": "image",
  "data": {
    "image": ImageData,
  }
}
```

### System Event

```js
{
  "ts": DateTime,
  "type": "system_event",
  "data": {
    "event": "startup"
  }
}
```

## Batch

```js
{
  "events": [Log]
}
```

## BatchData

```js
{
  "id": UUID,
  "start": DateTime,
  "end": DateTime,
  "start_hash": SHA256,
  "end_hash": SHA256,
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

```js
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

```js
{
  "email": "user@example.com",
  "password": "Argon2Hash",
  "name": "string" | undefined
}
```

Response `201`:

```js
{
  "user": {
    "id": "uuid",
    "email": "user@example.com",
    "name": "string" | undefined,
  },
  "access_token": AccessToken
}
```

Also sets `refresh_token` httpOnly cookie.

### `POST /login`

Request:

```js
{
  "email": "user@example.com",
  "password": Argon2Hash
}
```

Response `200`:

```js
{ "access_token": AccessToken }
```

Also sets `refresh_token` httpOnly cookie.

### `POST /logout`

Clears the `refresh_token` cookie.

Response: `204 No Content`.

### `POST /token`

Refreshes access token using `refresh_token` cookie.

Response `201`:

```js
{ "access_token": "AccessToken" }
```

### `GET /user` (auth required)

Response `200`:

```js
{
  "id": "uuid",
  "email": "user@example.com",
  "name": "string" or undefined,
  "e2ee_key": UserEncryptedData(Binary) | undefined,
}
```

### `PATCH /user` (auth required)

Request:

```js
{
  "name": "string" | undefined,
  "e2ee_key": UserEncryptedData(Binary) | undefined,
}
```

Response `200`:

```js
{ "ok": true }
```

---

## Devices

All device endpoints require `Authorization: Bearer <token>`.

### `GET /device`

List devices that a user has access to.

Response `200`:

```js
[
  {
    id: 'uuid',
    owner: 'uuid',
    name: 'My Laptop',
    platform: 'linux',
    last_seen_at: '...',
    last_upload_at: '...',
    status: 'online',
    enabled: true,
  },
];
```

### `PATCH /device/:id`

Request:

```js
{ "name": "New Name", "enabled": true }
```

Response `200`:

```js
{ "id": UUID, "updated": true }
```

---

## Batches and Logs

### `GET /data`

List batches and other logs. Query params: `device_id?`, `user?`, `cursor?`,
`limit?` (max 100).

Response `200`:

```js
{
  "batches": [
    {
      "device_id": UUID,
      ...BatchData,
    }
  ],
  "logs": [
    {
      "device_id": UUID,
      ...Log,
    }
  ],
  "next_cursor": DateTime
}
```

---

## Partners

Flow

1. User requests a partner
2. Web client checks for the public key `GET /pubkey?email=me@example.com`
3. If it's there, web client encrypts the `e2ee_key` with the public key
4. Web client sends `POST /partner`
5. Partner logs in or creates an account and sends `POST /partner/accept`
6. (If the public key didn't exist) The original user runs `PUT /partner/:id` and add the encrypted e2ee key.

### `GET /pubkey?email=[email]`

Gets the public key for a user (used to distribute the e2ee_key)

### `POST /partner`

Invite partner by email.

Request:

```js
{
  "email": "partner@example.com",
  "permissions": { "view_data": true },

  // If the account already exist
  "e2ee_key": PartnerEncryptedData(Binary) | undefined
}
```

Response `201`:

```js
{ "id": "uuid", "status": "pending" }
```

### `POST /partner/accept`

Request:

```js
{ "id": "partner_record_id" }
```

Response `200`:

```js
{ "id": "partner_record_id" }
```

### `GET /partner`

Response `200`:

```js
[
  {
    id: 'uuid',
    partner: {
      id: 'uuid',
      email: 'partner@example.com',
      name: 'string' | undefined,
    },
    status: 'accepted',
    permissions: { view_data: true },
    created_at: DateTime,
    e2ee_key: PartnerEncryptedData(binary) | undefined,
  },
];
```

### `PATCH /partner/:id` (auth required)

Owner can update.

Request:

```js
{
  "permissions": { "view_data": true },
  "e2ee_key" PartnerEncryptedData(Binary)
}
```

Response `200`:

```js
{ "id": UUID, "permissions": { "view_data": true } }
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

Authentication is done using the `Authorization: Bearer <token>` header.

All endpoints other than `POST /d/device` use the device token.

## Endpoints

### `POST /d/device`

This creates a new device. It requires a user access token for the account
(obtained with `/login`).

Request:

```js
{ "name": "My Laptop", "platform": "linux" }
```

Response `201`:

```js
{ "id": "uuid", "created_at": DateTime, "token": DeviceToken }
```

### `GET /d/device`

Returns the device info/settings.

```js
{
    "id": UUID,
    "name": "My Laptop",
    "platform": "linux",
    "enabled": true,
    "e2ee_key": Base64,
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

```js
{
  "batch": BatchData,
  "new_start_hash": SHA256
}
```

### `POST /d/logs`

Adds a non-batched log.

Request:

```js
{
  ...Log, // Without "id"
}
```

Response `201`:

```js
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

```js
{ "ok": true }
```

### `GET /hash`

Get the latest rolling state.

Content-Type: `application/octet-stream`

Response `200`

```
[hash:16B]
```
