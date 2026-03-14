# Virtue Initiative API Spec

Base URL examples:

- Local: `http://127.0.0.1:8787`
- Production: your deployed Workers URL

## Types

### Basic Types

- `UUID`: UUIDv4 string
- `DateTime`: millisecond Unix timestamp
- `Base64`: base64-encoded binary
- `SHA256`: lowercase hex-encoded SHA-256 digest
- `AccessToken`: JWT with `type: "access"` and `sub = user id`
- `DeviceAccessToken`: JWT with `type: "device-access"` and `sub = device id`
- `DeviceRefreshToken`: opaque string returned by `POST /d/device`
- `ServerToken`: JWT with `type: "server"` and `sub = device id`

### Errors

Standard error shape:

```js
{
  "error": "Unauthorized",
  "details": {}
}
```

Validation errors:

```js
{
  "error": "Bad Request",
  "details": {
    "errors": [],
    "properties": {
      "email": { "errors": ["Invalid email"] }
    }
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

## Shared Shapes

### User

```js
{
  "id": UUID,
  "email": "user@example.com",
  "email_verified": true,
  "email_bounced_at": DateTime | null,
  "name": "Name" | undefined,
  "e2ee_key": Base64 | undefined,
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

### Log

```js
{
  "ts": DateTime,
  "type": "system_event",
  "data": {
    "event": "startup"
  },
  "risk": 0.7 | undefined
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
  "url": "https://.../user/.../batches/...enc"
}
```

### Partner

```js
{
  "id": UUID,
  "role": "owner" | "invitee",
  "partner": {
    "id": UUID | undefined,
    "email": "partner@example.com",
    "name": "Partner Name" | undefined
  },
  "status": "pending" | "accepted",
  "permissions": {
    "view_data": true
  },
  "created_at": DateTime,
  "e2ee_key": Base64 | undefined
}
```

### Notification Preference

```js
{
  "partnership_id": UUID,
  "status": "pending" | "accepted",
  "monitored_user": {
    "id": UUID,
    "email": "owner@example.com",
    "name": "Owner Name" | undefined
  },
  "email_frequency": "none" | "alerts-only" | "daily" | "weekly",
  "immediate_tamper_severity": "warning" | "critical"
}
```

## Main API

### Authentication Model

- User access tokens go in `Authorization: Bearer <token>`.
- User refresh tokens are stored in the `refresh_token` httpOnly cookie.
- Current server defaults: access token TTL 1 hour, refresh token TTL 365 days.

### `GET /`

Health check.

Response `200`:

```js
{
  "name": "Virtue Initiative API",
  "version": "1.0.0",
  "status": "ok"
}
```

## Auth

### `POST /signup`

Request:

```js
{
  "email": "user@example.com",
  "password": "string",
  "name": "Name" | undefined
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
  "password": "string"
}
```

Response `200`:

```js
{
  "access_token": AccessToken
}
```

Also sets the `refresh_token` cookie.

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

Auth required.

Response `200`: [User](#user)

### `PATCH /user`

Auth required.

Request:

```js
{
  "email": "new@example.com" | undefined,
  "name": "Name" | undefined,
  "e2ee_key": Base64 | undefined,
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

### `POST /verify-email`

Request:

```js
{
  "token": "opaque token from email link"
}
```

Response `200`:

```js
{
  "ok": true,
  "email": "user@example.com"
}
```

### `POST /verify-email/request`

Auth required.

Response `200`:

```js
{
  "ok": true,
  "already_verified": true | undefined
}
```

### `POST /password-reset/request`

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
  "token": "opaque token from email link"
}
```

Response `200`:

```js
{
  "ok": true,
  "email": "user@example.com",
  "user_id": UUID,
  "key_rotation_required": true,
  "partner_access_targets": [
    {
      "partnership_id": UUID,
      "partner_email": "partner@example.com",
      "partner_pub_key": Base64 | undefined
    }
  ]
}
```

### `POST /password-reset`

Request:

```js
{
  "token": "opaque token from email link",
  "password": "string",
  "e2ee_key": Base64 | undefined,
  "pub_key": Base64 | undefined,
  "priv_key": Base64 | undefined,
  "partner_access_keys": [
    {
      "partnership_id": UUID,
      "e2ee_key": Base64
    }
  ] | undefined
}
```

Response `200`:

```js
{
  "ok": true
}
```

## Devices

All `/device/*` endpoints require a user access token.

### `GET /device`

Lists devices the current user can view, including accepted partners' devices when `view_data` is allowed.

Response `200`:

```js
[
  {
    "id": UUID,
    "owner": UUID,
    "name": "My Laptop",
    "platform": "linux",
    "last_upload_at": DateTime | null,
    "status": "online" | "offline",
    "enabled": true
  }
]
```

### `PATCH /device/:id`

Updates a device owned by the current user.

Request:

```js
{
  "name": "New Name" | undefined,
  "enabled": true | undefined
}
```

Response `200`:

```js
{
  "id": UUID,
  "updated": true
}
```

### `DELETE /device/:id`

Deletes a device owned by the current user, removes its batches, and may send notification emails.

Response: `204 No Content`

## Data

### `GET /data`

Auth required.

Lists uploaded batches and direct device logs for the current user, or for a monitored user if the requester has `view_data`.

Query params:

- `device_id?`
- `user?`
- `cursor?`
- `limit?` with max `100`

Response `200`:

```js
{
  "batches": [
    {
      "id": UUID,
      "device_id": UUID,
      "start_time": DateTime,
      "end_time": DateTime,
      "end_hash": SHA256,
      "url": "https://..."
    }
  ],
  "logs": [
    {
      "device_id": UUID,
      "ts": DateTime,
      "type": "system_event",
      "data": {
        "event": "startup"
      },
      "risk": 0.7 | undefined
    }
  ],
  "next_cursor": DateTime | undefined
}
```

## Partners

### `GET /pubkey?email=user@example.com`

Returns the public key for a user if present.

Response `200`:

```js
{
  "pubkey": Base64
}
```

### `POST /partner`

Auth required.

Creates a pending partnership and sends an email invite.

Request:

```js
{
  "email": "partner@example.com",
  "permissions": {
    "view_data": true
  },
  "e2ee_key": Base64 | undefined
}
```

Response `201`:

```js
{
  "id": UUID,
  "status": "pending"
}
```

### `POST /partner/invite/validate`

Validates an invite token before login or signup.

Request:

```js
{
  "token": "opaque invite token"
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

### `POST /partner/invite/accept`

Auth required.

Accepts a pending invite token for the current user.

Request:

```js
{
  "token": "opaque invite token"
}
```

Response `200`:

```js
{
  "id": UUID
}
```

### `GET /partner`

Auth required.

Response `200`:

```js
[
  {
    "id": UUID,
    "role": "owner" | "invitee",
    "partner": {
      "id": UUID | undefined,
      "email": "partner@example.com",
      "name": "Partner Name" | undefined
    },
    "status": "pending" | "accepted",
    "permissions": {
      "view_data": true
    },
    "created_at": DateTime,
    "e2ee_key": Base64 | undefined
  }
]
```

### `PATCH /partner/:id`

Auth required. Only the owner side may update.

Request:

```js
{
  "permissions": {
    "view_data": true
  } | undefined,
  "e2ee_key": Base64 | undefined
}
```

Response `200`:

```js
{
  "id": UUID,
  "permissions": {
    "view_data": true
  }
}
```

### `DELETE /partner/:id`

Auth required. Either the owner or the invitee side may delete if authorized.

Response: `204 No Content`

## Notifications

### `GET /notifications/preferences`

Auth required.

Lists notification preferences for partnerships where the current user is the invitee / monitoring partner.

Response `200`:

```js
[
  {
    "partnership_id": UUID,
    "status": "pending" | "accepted",
    "monitored_user": {
      "id": UUID,
      "email": "owner@example.com",
      "name": "Owner Name" | undefined
    },
    "email_frequency": "none" | "alerts-only" | "daily" | "weekly",
    "immediate_tamper_severity": "warning" | "critical"
  }
]
```

### `PATCH /notifications/preferences/:id`

Auth required.

Request:

```js
{
  "email_frequency": "none" | "alerts-only" | "daily" | "weekly" | undefined,
  "immediate_tamper_severity": "warning" | "critical" | undefined
}
```

Response `200`:

```js
{
  "partnership_id": UUID,
  "email_frequency": "none" | "alerts-only" | "daily" | "weekly",
  "immediate_tamper_severity": "warning" | "critical"
}
```

## Email Webhooks

### `POST /email/sns`

Handles AWS SNS subscription confirmations plus SES bounce and complaint notifications.

For SNS `SubscriptionConfirmation`, the worker calls `SubscribeURL`.

For SES `Notification` envelopes, the worker:

- marks bounced users as email-bounced
- marks bounced and complained users as unverified

Response `200`:

```js
{
  "ok": true,
  "subscribed": true | undefined,
  "updated": 2 | undefined
}
```

## Public R2 Pass-through

### `GET /r2/*`

Returns encrypted batch blob bytes from R2 with no auth.

Response content type: `application/octet-stream`

## Device API

### Authentication

- `POST /d/device` uses a user `AccessToken`
- `POST /d/token` uses a device refresh token in the request body
- `GET /d/device`, `POST /d/batch`, `POST /d/log`, and `POST /hash` / `GET /hash` use a `DeviceAccessToken`
- `DELETE /hash` uses a `ServerToken`

Current server defaults:

- Device access token TTL: 7 days
- Device refresh token TTL: `1000 * 365` days, multiplied by seconds again in current server code, so effectively very long-lived

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

Returns the authenticated device's settings.

Response `200`:

```js
{
  "id": UUID,
  "name": "My Laptop",
  "platform": "linux",
  "enabled": true,
  "e2ee_key": Base64 | undefined,
  "hash_base_url": "https://hash-server.example.com"
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

Uploads an encrypted batch as multipart form data.

Fields:

- `file`: encrypted batch blob
- `start_time`: batch window start timestamp
- `end_time`: batch window end timestamp

Response `201`:

```js
{
  "id": UUID,
  "start_time": DateTime,
  "end_time": DateTime,
  "end_hash": SHA256,
  "url": "https://..."
}
```

### `POST /d/log`

Uploads a direct log entry.

Request:

```js
{
  "ts": DateTime,
  "type": "service_stop",
  "risk": 1 | undefined,
  "data": {
    "title": "Monitoring stopped unexpectedly" | undefined,
    "details": "..." | undefined
  }
}
```

Response `201`:

```js
{
  "ts": DateTime,
  "type": "service_stop",
  "risk": 1 | undefined,
  "data": {}
}
```

## Hash API

For each uploaded content hash:

```txt
new_state = sha256(current_state || content_hash)
```

### `POST /hash`

Auth: `DeviceAccessToken`

Request body:

- content type: `application/octet-stream`
- body: exactly 32 bytes

Response `200`:

```js
{
  "ok": true
}
```

### `GET /hash`

Auth: `DeviceAccessToken`

Response `200`:

- content type: `application/octet-stream`
- body: current 32-byte rolling hash state

### `DELETE /hash`

Auth: `ServerToken`

Resets a device's rolling hash state to zero bytes.

Response `200`:

```js
{
  "ok": true
}
```
