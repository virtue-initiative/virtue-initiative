# Virtue Initiative API Server

## 1. Overview

This spec defines the main API server for Virtue Initiative. It handles users, devices and batches.

### 1.1 Types

**Basic Types**

- `UUID`: UUIDv4 string
- `DateTime`: millisecond Unix timestamp
- `Base64`: base64-encoded binary
- `SHA256`: lowercase hex-encoded SHA-256 digest
- `RefreshToken`: opaque web-session string, prefixed by purpose (e.g. `wst_...`). Set as the HTTPOnly `refresh_token` cookie by `POST /login`, `POST /signup`, and `POST /email-verification/validate`.
- `HashServerToken`: EdDSA JWT (`Ed25519`) with `type: "device"` and `sub = device id`; minted by `POST /d/device`, `GET /d/device`, and `POST /d/batch`
- `DeviceRefreshToken`: opaque string returned by `POST /d/device`
- `ServerToken`: EdDSA JWT (`Ed25519`) with `type: "server"` and `sub = device id`

**HashParams**

```json
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

**User**

```js
{
  "id": UUID,
  "email": "user@example.com",
  "email_verified": true,
  "email_bounced_at": DateTime | null,
  "settings": {
    "email_frequency": "none" | "alerts-only" | "daily" | "weekly",
    "timezone": "America/New_York"
  },
  "name": "Name" | undefined,
  "pub_key": Base64 | undefined,
  "encrypted_priv_key": Base64 | undefined
}
```

**Device**

```js
{
  "id": UUID,
  "owner": UUID,
  "name": "My Laptop",
  "platform": "linux",
  "last_upload_at": DateTime | null,
  "last_hash_at": DateTime | null,
  "pending_count": Number,
  "status": "online" | "offline" | "logged_out"
}
```

**BatchData**

```js
{
  "id": UUID,
  "device_id": UUID,
  "start_time": DateTime,
  "end_time": DateTime,
  "end_hash": SHA256,
  "version": "v0.1" | "v1" | "...",
  "url": "https://.../user/.../batches/...enc",
  "encrypted_key": Base64,
  "created_at": DateTime
}
```

**PartnerInfo**

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

**DeviceSettings**

```js
{
  "id": UUID,
  "name": "My Laptop",
  "platform": "linux",
  "wrapping_keys": [
    {
      "user_id": UUID,
      "pub_key": Base64
    }
  ],
  "hash_base_url": "https://..." | null,
  "hash_token": HashServerToken
}
```

### 1.2 Authentication

The client MUST provide a refresh token in the Authorization header or a `refresh_token` cookie to access authenticated routes.

Additionally, the client SHOULD use a cookie store to store any short lived JWT access token the server MAY provide.

The server MUST reject any request that is missing a refresh token or a JWT cookie with **HTTP 401**. The server MAY reject any request with an invalid session token (even if it contains a valid cookie).

There are **TWO** types of tokens:

1. **Web Tokens**, these are the normal tokens, granted on `POST /login` and `POST /signup`
2. **Device Tokens**, these are device tokens, granted on `POST /d/device` and have a limited scope.

### 1.3 Validation

The server SHOULD validate every request shape against a schema and return **HTTP 400** on failure.

### 1.4 Backwards compat

The client MUST accept extra fields in the response body.

The client MUST prefixed the API with the current major version. For versions before `v1`, use `v0.x`. The server SHOULD return **HTTP 410 Gone** if it no longer supports a version.

```
  /api/v0.1/...
  /api/v0.2/...
  /api/v1/...
  /api/v2/...
```

The API version SHOULD match the major version of the codebase (see client/version.properties).

### 1.5 Status codes

The server SHOULD return **HTTP 204** for responses with no body, and **HTTP 200** otherwise —
including for endpoints that create a resource.

### 1.6 `GET /` - Health

MUST NOT require authentication. MUST return this shape:

```json
{
  "name": "Virtue Initiative API",
  "version": "1.0.0",
  "commit": "51e8a2690a19adcfdbd62494cc8b2b83f24c560b",
  "status": "ok"
}
```

The version SHOULD match the version of the codebase.

## 2. Users and authentication

### 2.1 `POST /signup-request`

The client MUST provide this shape.

```js
{
  "email": "user@example.com",
  "partner_invite_token": "opaque-string" | undefined
}
```

If a user at that email does not exist, the server MUST send a signup link to that email.

If a user at that email already exists, the server MUST send a notification to that email saying that their account already exists.

In BOTH cases, the server MUST respond **HTTP 204**.

### 2.2 `POST /signup`

The client MUST provide this request shape.

```js
{
  "verification_token": "opaque-string",
  "password_auth": Base64,
  "password_salt": Base64,
  "pub_key": Base64,
  "encrypted_priv_key": Base64,
  "name": "Name" | undefined,
  "email_digest_minutes_utc": Number | undefined
}
```

The server SHOULD verify that `password_auth`, `password_salt` and `pub_key` are all valid.

The server SHOULD reject an invalid `verification_token` with **HTTP 401**. Note: A `verification_token` becomes invalid as soon as any account with that email is created.

The server MUST return a `refresh_token` cookie containing the refresh token.

The server MUST create an account for the user and return **HTTP 200** with this shape.

```js
{
  "user": {
    "id": UUID,
    "email": "user@example.com",
    "email_verified": true,
    "name": "Name" | undefined
  }
}
```

### 2.3 `POST /password-reset`

The client MUST send the email to request a password reset:

```js
{
  "email": "user@example.com"
}
```

If the account exists, the server SHOULD send a password reset email, otherwise it SHOULD send an account does not exist email.

The server SHOULD always respond with **HTTP 204**.

### 2.4 `POST /password-reset/validate`

The client MUST send a password-reset token.

```js
{
  "token": "opaque-string",
}
```

The server MUST respond **HTTP 401** if the token is invalid or not found.

The server MUST respond **HTTP 200** with the account email.

```js
{
  "email": "user@example.com"
}
```

### 2.5 `POST /password-reset/finalize`

The client MUST send the follow request data.

```js
{
  "token": "opaque-string",
  "password_auth": Base64,
  "password_salt": Base64,
  "pub_key": Base64,
  "encrypted_priv_key": Base64
}
```

The server SHOULD validate the `password_auth`, `password_salt` and `pub_key` in the same way as `/signup`

The server MUST respond **HTTP 401** if the token is invalid or not found.

The server MUST update the user with the new information and return **HTTP 204**.

### 2.6 `GET /user/login-material`

The client MAY send an email as a query param `?email=[email]`.

The server MUST return an indistinguishable response for existing and non-existing emails.

If the client did not provide an email, the server MUST respond (**HTTP 200**) with the current argon2 hashing parameters:

```js
{
  "params": HashParams
}
```

If the client did provide an email, the server MUST additionally respond with the salt for that user. It MUST provide a made up salt if the user does not exist.

```js
{
  "password_salt": Base64,
  "params": HashParams
}
```

### 2.7 `POST /login`

The client MUST send this request shape

```js
{
  "email": "user@example.com",
  "password_auth": Base64,
  "timezone": "America/New_York" | undefined
}
```

The server MUST check if the user exists and then check the `password_auth` against the database and return **HTTP 401** if it doesn't match.

If the email has been marked as unverified, the server SHOULD send a verification email and return `403` with

```
  { "error": "Please verify your email before logging in. A verification email has been sent." }
```

If it matches, the server MUST send **HTTP 204** and set the `refresh_token` cookie.

### 2.8 `POST /email-verification/validate`

The client MUST provide a email token in the body.

```js
{
  "token": "opaque-string"
}
```

The server MUST return **HTTP 401** on an unknown or expired token.

The server MUST mark the email as verified and, if it is a email change request, change the email.

The server MUST respond with **HTTP 200**:

```js
{
  "email": "user@example.com",
  "purpose": "email_change" | "email_verification"
}
```

### 2.9 `POST /logout`

The client MUST authenticate with a **Web Token**.

The server MUST invalidate the session and clear the refresh_token.

The server MUST respond with **HTTP 204**.

### 2.7 `GET /user`

The client MUST authenticate with either token.

The server MUST return the currently authenticated user.

```js
User;
```

### 2.10 `PATCH /user`

The client MUST authenticate with a **Web Token**.

The client MUST provide a request in this shape.

```js
{
  "email": "new@example.com" | undefined,
  "name": "New Name" | undefined,
  "email_frequency": "none" | "alerts-only" | "daily" | "weekly" | undefined,
  "pub_key": Base64 | undefined,
  "encrypted_priv_key": Base64 | undefined
}
```

The server SHOULD NOT change the email, instead it should send an `email_change` token to the new email address. If the email address is being used by an existing account, it MUST return the same response, but send a "Email already in use." email to the new email.

The server SHOULD always update all the other fields in the database.

The server MUST return this response shape.

```js
{
  "ok": true,
  "email_verification_required": true | undefined,
  "pending_email": "new@example.com" | undefined
}
```

### 2.11 `DELETE /user?confirm_email=[email]`

The client MUST be authenticated with a **Web Token**.

The client MUST send the user's email in the query. The server SHOULD respond with **HTTP 400** if the email does not match.

If the email matches, the server SHOULD permanently delete the account. The server SHOULD delete all devices, batches, sessions, and tokens. The server SHOULD NOT delete the batch data in R2, instead it should be deleted by the normal 30 day cycle.

## 3. Partners

### 3.1 `POST /partner`

The client MUST authenticate with a **Web Token** and send

```js
{
  "email": "partner@example.com"
}
```

The server SHOULD send a partner request email to the provided email address.

The server SHOULD then respond **HTTP 200**

```js
{
  "id": UUID,
  "status": "pending"
}
```

### 3.2 `POST /partner/validate`

To get the information about an invite (often before signing up), the client MUST send.

```js
{
  "token": "opaque-string"
}
```

The server MUST return **HTTP 401** for an invalid or unknown token.

For a correct token, server MUST return **HTTP 200**.

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

### 3.3 `POST /partner/accept`

The client MUST be authenticated with a **Web Token** and send

```js
{
  "token": "opaque-string"
}
```

The server MUST finalize the partnership and return **HTTP 200**:

```js
{
  "id": UUID
}
```

### 3.4 `GET /partner`

The client MUST be authenticated with a **Web Token.**

The server MUST collect the lists of partners for the user and return **HTTP 200** with the below shape:

```js
{
  "watching": PartnerInfo[],
  "watchers": PartnerInfo[]
}
```

### 3.5 `DELETE /partner/:id`

The client MUST be authenticated with a **Web Token**.

The id MUST be a valid partnership ID.

The server MUST delete the partnership and notify the other user.

The server MUST respond **HTTP 204**.

## 4. Device Management

### 4.1 `GET /device`

The client MUST be authenticated with a **Web Token**.

The server SHOULD fetch the count from the hash server and the last updated from the hash server.

The server SHOULD return **HTTP 200** plus a list of all the devices that a user can view.

```js
Device[]
```

### 4.2 `PATCH /device/:id`

The client MUST be authenticated with a **Web Token** and send

```js
{
  "name": "New Device Name"
}
```

On success, the server MUST respond with **HTTP 204**.

### 4.3 `DELETE /device/:id`

The client MUST be authenticated with a **Web Token**.

The server SHOULD return **HTTP 404** if the device does not exist or is not owned by the user.

The server SHOULD delete the device with the stored batches and email the owner and watchers.

On success, the server SHOULD return `204 No Content`.

## 5. Data

### 5.1 `GET /data`

The client MUST be authenticated wit ha **Web Token**

The client MAY send `?since=DateTime` to only receive batches since that time. The client MAY provide `?since=0` to get all batches.

The server MUST collect the user, partners, and batches and return them in this shape.

```js
{
  "batches": BatchData[],
  "batches_complete": true,
  "watching": PartnerInfo[],
  "watchers": PartnerInfo[],
  "user": User,
}
```

The server SHOULD filter the batches to only include batches the user can decrypt.

The server MAY return a subset of the batches and mark "batches_complete": false. The server MUST return oldest batches first.

The server MUST only return batches where `created_at > since`.

## 6. Device Only

### 6.1 `POST /d/device`

The client MUST send

```js
{
  "email": "user@example.com",
  "password_auth": Base64,
  "name": "My Laptop",
  "platform": "linux"
}
```

The server MUST validate the login (see `POST /login`) and create a long lived refresh token.

It should also return the device settings (see `GET /d/device`)

On success, it must return **HTTP 200** with

```js
{
  "token": DeviceRefreshToken,
  "settings": DeviceSettings
}
```

### 6.2 `GET /d/device`

The client MUST be authenticated with a **Device Token**.

The server SHOULD create a fresh JWT hash server token (see the hash server spec) and return **HTTP 200** with

```js
DeviceSettings;
```

### 6.3 `POST /d/logout`

The client MUST be authenticated with a **Device Token**.

The server MUST revoke the device token and soft-delete the device. It also resets the device hash state.

On success, the server MUST respond with **HTTP 204**

### 6.4 `POST /d/batch`

The client MUST send a multipart form request:

- `metadata`: JSON
  - ```js
    {
      "start_time": DateTime,
      "end_time": DateTime,
      "access_keys`: {
        "<user_id UUID>": Base64 // hpke_key
      },
      "event_counts": {
        "total": non-negative int,
        "high": non-negative int,
        "medium": non-negative int,
        "screenshot": non-negative int,
      },
      "notifications": [
        {
          ts: DateTime,
          type: 'system_event',
          risk: 0.7,
          title: 'Device reported system event.' | undefined,
          details: '...' | undefined
        }
      ]
    }
    ```
- `file`: encrypted batch blob

The server MUST upload the file to object storage and store the metadata in the database.

The server SHOULD store the current major version in the database alongside the encrypted batch (for versioning the encrypted batch).

The server MUST return **HTTP 200** with the device's refreshed settings alongside the created batch:

```js
{
  "id": UUID,
  "start_time": DateTime,
  "end_time": DateTime,
  "end_hash": SHA256,
  "url": "https://.../user/.../batches/...enc",
  "settings": DeviceSettings
}
```

## 7. Other

### 7.1 `GET /r2/*`

This forwards to R2. Only used in dev. It SHOULD be disabled in production.

### 7.2 `POST /email/sns`

The server MUST handle any AWS SNS webhook.

The server MUST confirm the subscription (`SubscriptionConfirmation`)

The server SHOULD process `Bounce`/`Complaint` notifications by marking the users' emails as bounced and unverified.

The server MUST respond **HTTP 200** with this shape.

```js
{ "ok": true, "subscribed": true }   // SubscriptionConfirmation
{ "ok": true, "updated": Number }    // Bounce/Complaint notification
{ "ok": true }                        // ignored event
```
