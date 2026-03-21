# Virtue Initiative API Spec

Base URL examples:

- Local: `http://127.0.0.1:8787`
- Production: your deployed Workers URL

## Types

- `UUID`: UUIDv4 string
- `DateTime`: millisecond Unix timestamp
- `Base64`: base64-encoded binary
- `SHA256`: lowercase hex-encoded SHA-256 digest
- `AccessToken`: JWT with `type: "access"` and `sub = user id`
- `DeviceAccessToken`: JWT with `type: "device-access"` and `sub = device id`
- `DeviceRefreshToken`: opaque string returned by `POST /d/device`
- `ServerToken`: JWT with `type: "server"` and `sub = device id`

## Shared Shapes

### HashParams

```js
{
  "version": "argon2id-v1",
  "algorithm": "argon2id",
  "memory_cost_kib": 131072,
  "time_cost": 5,
  "parallelism": 1,
  "salt_length": 16,
  "hkdf_hash": "sha256"
}
```

### User

```js
{
  "id": UUID,
  "email": "user@example.com",
  "email_verified": true,
  "email_bounced_at": DateTime | null,
  "name": "Name" | undefined,
  "pub_key": Base64 | undefined,
  "priv_key": Base64 | undefined
}
```

### Device

```js
{
  "id": UUID,
  "owner": UUID,
  "name": "My Laptop",
  "platform": "linux",
  "last_upload_at": DateTime | null,
  "status": "online" | "offline",
  "enabled": true
}
```

### BatchData

```js
{
  "id": UUID,
  "device_id": UUID,
  "start_time": DateTime,
  "end_time": DateTime,
  "end_hash": SHA256,
  "url": "https://.../user/.../batches/...enc",
  "encrypted_key": Base64
}
```

### Partner List Item

```js
{
  "id": UUID,
  "user": {
    "id": UUID | undefined,
    "email": "partner@example.com",
    "name": "Partner Name" | undefined
  },
  "status": "pending" | "accepted",
  "created_at": DateTime
}
```

## Auth

### `GET /current-hash-params`

Returns the current client password-derivation settings.

Response `200`:

```js
HashParams;
```

### `GET /user/login-material?email=user@example.com`

Returns a login salt and the current hash params. The response shape is the same for existing and
non-existing users.

Response `200`:

```js
{
  "password_salt": Base64,
  "params": HashParams
}
```

### `POST /signup`

Request:

```js
{
  "email": "user@example.com",
  "name": "Name" | undefined,
  "password_auth": Base64,
  "password_salt": Base64,
  "pub_key": Base64,
  "priv_key": Base64
}
```

Response `201`:

```js
{
  "user": {
    "id": UUID,
    "email": "user@example.com",
    "email_verified": false,
    "name": "Name" | undefined
  },
  "access_token": AccessToken
}
```

Also sets the `refresh_token` cookie.

### `POST /login`

Request:

```js
{
  "email": "user@example.com",
  "password_auth": Base64
}
```

Response `200`:

```js
{
  "access_token": AccessToken
}
```

### `POST /logout`

Clears the `refresh_token` cookie.

Response: `204 No Content`

### `POST /token`

Refreshes a user access token from the `refresh_token` cookie.

Response `201`:

```js
{
  "access_token": AccessToken
}
```

### `GET /user`

Requires a user `AccessToken`.

Response `200`:

```js
User;
```

### `PATCH /user`

Requires a user `AccessToken`.

Request:

```js
{
  "email": "new@example.com" | undefined,
  "name": "New Name" | undefined,
  "pub_key": Base64 | undefined,
  "priv_key": Base64 | undefined
}
```

Response `200`:

```js
{
  "ok": true
}
```

### `POST /password-reset`

Request:

```js
{
  "email": "user@example.com"
}
```

Response: `204 No Content`

### `POST /password-reset/validate`

Request:

```js
{
  "token": "opaque-string"
}
```

Response `200`:

```js
{
  "ok": true,
  "email": "user@example.com"
}
```

### `POST /password-reset/finalize`

Request:

```js
{
  "token": "opaque-string",
  "password_auth": Base64,
  "password_salt": Base64,
  "pub_key": Base64,
  "priv_key": Base64
}
```

Response `200`:

```js
{
  "ok": true
}
```

## Partners

### `POST /partner`

Requires a user `AccessToken`.

Request:

```js
{
  "email": "partner@example.com"
}
```

Response `201`:

```js
{
  "id": UUID,
  "status": "pending"
}
```

### `POST /partner/validate`

Validates an invite token before login or signup.

Request:

```js
{
  "token": "opaque-string"
}
```

Response `200`:

```js
{
  "ok": true,
  "partnership_id": UUID,
  "owner": {
    "id": UUID,
    "email": "owner@example.com",
    "name": "Owner Name" | undefined
  }
}
```

### `POST /partner/accept`

Requires a user `AccessToken`.

Request:

```js
{
  "token": "opaque-string"
}
```

Response `200`:

```js
{
  "id": UUID
}
```

### `GET /partner`

Requires a user `AccessToken`.

Response `200`:

```js
{
  "watching": [
    {
      "id": UUID,
      "user": {
        "id": UUID,
        "email": "owner@example.com",
        "name": "Owner Name" | undefined
      },
      "status": "pending" | "accepted",
      "digest_cadence": "none" | "alerts-only" | "daily" | "weekly",
      "immediate_tamper_severity": "warning" | "critical",
      "created_at": DateTime
    }
  ],
  "watchers": [
    {
      "id": UUID,
      "user": {
        "id": UUID | undefined,
        "email": "partner@example.com",
        "name": "Partner Name" | undefined
      },
      "status": "pending" | "accepted",
      "created_at": DateTime
    }
  ]
}
```

### `PATCH /partner/watching/:id`

Requires a user `AccessToken`.

Request:

```js
{
  "digest_cadence": "none" | "alerts-only" | "daily" | "weekly" | undefined,
  "immediate_tamper_severity": "warning" | "critical" | undefined
}
```

Response: `204 No Content`

### `DELETE /partner/watcher/:id`

Requires a user `AccessToken`.

Response: `204 No Content`

### `DELETE /partner/watching/:id`

Requires a user `AccessToken`.

Response: `204 No Content`

## Data

### `GET /data`

Requires a user `AccessToken`.

Query parameters:

- `device_id`: optional `UUID`
- `user`: optional `UUID`
- `cursor`: optional timestamp
- `limit`: optional integer, max `100`

Response `200`:

```js
{
  "batches": [BatchData],
  "logs": [
    {
      "device_id": UUID,
      "ts": DateTime,
      "type": "system_event",
      "data": {},
      "risk": 0.7 | undefined
    }
  ],
  "next_cursor": DateTime | undefined
}
```

`batches` only include rows where the requester has a matching `encrypted_key` envelope.

## Device API

The following routes use device auth:

- `POST /d/device` uses a user `AccessToken`
- `POST /d/token` uses a `DeviceRefreshToken`
- `GET /d/device`, `POST /d/batch`, `POST /d/log`, `POST /hash`, `GET /hash`, and `DELETE /hash`
  use a `DeviceAccessToken` or `ServerToken` as applicable

### `POST /d/device`

Registers a device for the authenticated user.

Request:

```js
{
  "name": "My Laptop",
  "platform": "linux"
}
```

Response `201`:

```js
{
  "id": UUID,
  "access_token": DeviceAccessToken,
  "refresh_token": DeviceRefreshToken
}
```

### `GET /d/device`

Response `200`:

```js
{
  "id": UUID,
  "name": "My Laptop",
  "platform": "linux",
  "enabled": true,
  "owner": {
    "user_id": UUID,
    "pub_key": Base64
  } | undefined,
  "partners": [
    {
      "user_id": UUID,
      "pub_key": Base64
    }
  ],
  "hash_base_url": "https://..." | null
}
```

### `POST /d/token`

Request:

```js
{
  "refresh_token": DeviceRefreshToken
}
```

Response `200`:

```js
{
  "access_token": DeviceAccessToken
}
```

### `POST /d/batch`

Multipart form request:

- `start_time`: integer
- `end_time`: integer
- `access_keys`: JSON string
- `file`: encrypted batch blob

`access_keys` JSON shape:

```js
{
  "keys": [
    {
      "user_id": UUID,
      "hpke_key": Base64
    }
  ]
}
```

Response `201`:

```js
{
  "id": UUID,
  "start_time": DateTime,
  "end_time": DateTime,
  "end_hash": SHA256,
  "url": "https://.../user/.../batches/...enc"
}
```

### `POST /d/log`

Request:

```js
{
  "ts": DateTime,
  "type": "system_event",
  "risk": 0.7 | undefined,
  "data": {}
}
```

Response `201` echoes the stored log.

## Hash API

### `POST /hash`

Uploads a single 32-byte plaintext content hash for the device hash chain.

Response `200`:

```js
{
  "ok": true
}
```

### `GET /hash`

Returns the current 32-byte hash-chain state as binary.

### `DELETE /hash`

Requires a `ServerToken`.

Resets the current device hash-chain state.
