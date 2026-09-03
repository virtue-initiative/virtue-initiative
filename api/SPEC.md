# Virtue Initiative API Server

## API-001 Overview

This spec defines the main API server for Virtue Initiative. It handles users, devices and batches.

### API-002 Types

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

### API-003 Authentication

The client MUST provide a refresh token in the Authorization header or a `refresh_token` cookie to access authenticated routes.

Additionally, the client SHOULD use a cookie store to store any short lived JWT access token the server MAY provide.

The server MUST reject any request that is missing a refresh token or a JWT cookie with **HTTP 401**. The server MAY reject any request with an invalid session token (even if it contains a valid cookie).

There are **TWO** types of tokens:

1. **Web Tokens**, these are the normal tokens, granted on `POST /login` and `POST /signup`
2. **Device Tokens**, these are device tokens, granted on `POST /d/device` and have a limited scope.

### API-004 Validation

The server SHOULD validate every request shape against a schema and return **HTTP 400** on failure.

### API-005 Backwards compat

The client MUST accept extra fields in the response body.

The client MUST prefixed the API with the current major version. For versions before `v1`, use `v0.x`. The server SHOULD return **HTTP 410 Gone** if it no longer supports a version.

```
  /api/v0.1/...
  /api/v0.2/...
  /api/v1/...
  /api/v2/...
```

The API version SHOULD match the major version of the codebase (see client/version.properties).

### API-006 Status codes

The server SHOULD return **HTTP 204** for responses with no body, and **HTTP 200** otherwise —
including for endpoints that create a resource.

### API-007 `GET /` - Health

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

## API-008 Users and authentication

### API-009 `POST /signup-request`

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

### API-010 `POST /signup`

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

### API-011 `POST /signup/validate`

The client MUST send a signup verification token.

```js
{
  "token": "opaque-string",
}
```

The server MUST respond **HTTP 400** if the token is invalid, expired, or an account already exists for its email.

The server MUST respond **HTTP 200** with the pending signup's email.

```js
{
  "email": "user@example.com"
}
```

### API-012 `POST /password-reset`

The client MUST send the email to request a password reset:

```js
{
  "email": "user@example.com"
}
```

If the account exists, the server SHOULD send a password reset email, otherwise it SHOULD send an account does not exist email.

The server SHOULD always respond with **HTTP 204**.

### API-013 `POST /password-reset/validate`

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

### API-014 `POST /password-reset/finalize`

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

### API-015 `GET /user/login-material`

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

### API-016 `POST /login`

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

### API-017 `POST /email-verification/validate`

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

### API-018 `POST /logout`

The client MUST authenticate with a **Web Token**.

The server MUST invalidate the session and clear the refresh_token.

The server MUST respond with **HTTP 204**.

### API-019 `GET /user`

The client MUST authenticate with either token.

The server MUST return the currently authenticated user.

```js
User;
```

### API-020 `PATCH /user`

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

### API-021 `DELETE /user?confirm_email=[email]`

The client MUST be authenticated with a **Web Token**.

The client MUST send the user's email in the query. The server SHOULD respond with **HTTP 400** if the email does not match.

If the email matches, the server SHOULD permanently delete the account. The server SHOULD delete all devices, batches, sessions, and tokens. The server SHOULD NOT delete the batch data in R2, instead it should be deleted by the normal 30 day cycle.

## API-022 Partners

### API-023 `POST /partner`

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

### API-024 `POST /partner/validate`

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

### API-025 `POST /partner/accept`

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

### API-026 `GET /partner`

The client MUST be authenticated with a **Web Token.**

The server MUST collect the lists of partners for the user and return **HTTP 200** with the below shape:

```js
{
  "watching": PartnerInfo[],
  "watchers": PartnerInfo[]
}
```

### API-027 `DELETE /partner/:id`

The client MUST be authenticated with a **Web Token**.

The id MUST be a valid partnership ID.

The server MUST delete the partnership and notify the other user.

The server MUST respond **HTTP 204**.

## API-043 Locked passwords

A locked password lets a user (the owner) store a secret they don't want easy access to themselves, e.g. a Screen Time passcode. Reading it is treated as a red flag: the entry is permanently marked accessed and every accepted watcher is emailed immediately, without exposing the value to them. Watchers otherwise have no visibility into it at all.

The value MUST be end-to-end encrypted the same way batch keys are (see `access_keys`), sealed with HPKE for the owner's own `pub_key`.

### API-044 `POST /locked-password`

The client MUST authenticate with a **Web Token**.

The client MUST send:

```js
{
  "label": "Screen Time passcode",
  "wrapped_value": Base64 // HPKE-sealed for the caller's own pub_key
}
```

The server MUST store the entry against the caller as owner and respond **HTTP 200**:

```js
{
  "id": UUID
}
```

### API-045 `GET /locked-password`

The client MUST authenticate with a **Web Token**.

The server MUST return every entry the caller owns, including soft-deleted ones (the client decides how to display them). The server MUST NOT include `wrapped_value` in this list.

```js
[
  {
    "id": UUID,
    "label": "Screen Time passcode",
    "created_at": DateTime,
    "accessed_at": DateTime | null,
    "deleted_at": DateTime | null
  }
]
```

### API-046 `POST /locked-password/:id/reveal`

The client MUST authenticate with a **Web Token**. The caller MUST be the entry's owner, else **HTTP 404**.

The server MUST respond **HTTP 200**:

```js
{
  "wrapped_value": Base64,
  "accessed_at": DateTime | null
}
```

If `accessed_at` was previously null, the server MUST permanently set it to now (no endpoint may clear it) and MUST immediately email every accepted watcher that this password was accessed, naming it by its `label` but never including its value, using the same immediate-alert delivery as a tamper alert (skipping watchers whose `email_frequency` is `none`).

### API-047 `DELETE /locked-password/:id`

The client MUST authenticate with a **Web Token**. The caller MUST be the entry's owner, else **HTTP 404**.

The server MUST soft-delete the entry by setting `deleted_at` to now and respond **HTTP 204**. The server SHOULD hard-delete any entry whose `deleted_at` is more than 7 days old.

### API-048 `POST /locked-password/:id/restore`

The client MUST authenticate with a **Web Token**. The caller MUST be the entry's owner, else **HTTP 404**.

The server MUST null out `deleted_at` and respond **HTTP 204**.

### API-049 `DELETE /locked-password/:id/permanent`

The client MUST authenticate with a **Web Token**. The caller MUST be the entry's owner, else **HTTP 404**.

The server MUST hard-delete the entry regardless of its current `deleted_at` and respond **HTTP 204**.

## API-028 Device Management

### API-029 `GET /device`

The client MUST be authenticated with a **Web Token**.

The server SHOULD fetch the count from the hash server and the last updated from the hash server.

The server SHOULD return **HTTP 200** plus a list of all the devices that a user can view.

```js
Device[]
```

### API-030 `PATCH /device/:id`

The client MUST be authenticated with a **Web Token** and send

```js
{
  "name": "New Device Name"
}
```

On success, the server MUST respond with **HTTP 204**.

### API-031 `DELETE /device/:id`

The client MUST be authenticated with a **Web Token**.

The server SHOULD return **HTTP 404** if the device does not exist or is not owned by the user.

The server SHOULD delete the device with the stored batches and email the owner and watchers.

On success, the server SHOULD return `204 No Content`.

## API-032 Data

### API-033 `GET /data`

The client MUST be authenticated wit ha **Web Token**

The client MAY send `?since=DateTime` to only receive batches since that time.

The server MUST collect the user, partners, and batches and return them in this shape.

```js
{
  "batches": BatchData[],
  "watching": PartnerInfo[],
  "watchers": PartnerInfo[],
  "user": User,
}
```

The server SHOULD filter the batches to only include batches the user can decrypt.

The server MUST only return batches where `created_at > since`.

## API-034 Device Only

### API-035 `POST /d/device`

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

### API-036 `GET /d/device`

The client MUST be authenticated with a **Device Token**.

The server SHOULD create a fresh JWT hash server token (see the hash server spec) and return **HTTP 200** with

```js
DeviceSettings;
```

### API-037 `POST /d/logout`

The client MUST be authenticated with a **Device Token**.

The server MUST revoke the device token and soft-delete the device. It also resets the device hash state.

Because a logout removes a device from monitoring, the server SHOULD email the owner and the owner's accepted watchers (skipping watchers whose `email_frequency` is `none`) that the device logged out. Email delivery MUST be best-effort: a failure to send MUST NOT fail the request.

On success, the server MUST respond with **HTTP 204**

### API-038 `POST /d/batch`

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

## API-039 Other

### API-040 `GET /r2/*`

This forwards to R2. Only used in dev. It SHOULD be disabled in production.

### API-041 `POST /email/sns`

The server MUST handle any AWS SNS webhook.

The server MUST confirm the subscription (`SubscriptionConfirmation`)

The server SHOULD process `Bounce`/`Complaint` notifications by marking the users' emails as bounced and unverified.

The server MUST respond **HTTP 200** with this shape.

```js
{ "ok": true, "subscribed": true }   // SubscriptionConfirmation
{ "ok": true, "updated": Number }    // Bounce/Complaint notification
{ "ok": true }                        // ignored event
```

### API-042 `POST /bug-report`

The client MAY authenticate with a **Web Token** or a **Device Token**. The client MAY also send no token at all.

The client MUST send a multipart form request:

- `metadata`: JSON
  - ```js
    {
      "message": "Description of the problem",
      "contact_email": "user@example.com" | undefined,
      "platform": "linux" | "windows" | "mac" | "android" | "ios" | "web" | undefined,
      "app_version": "1.2.3" | undefined,
      "platform_details": "Linux 6.8.0-60-lowlatency; Ubuntu 24.04" | undefined
    }
    ```
- `log_file`: recent client log excerpt (e.g. plain text), undefined if the client has none to offer

The client SHOULD omit `contact_email` when authenticated with a token.

The client SHOULD gather OS/kernel details and send it in `platform_details`.

If the client omits it, the server SHOULD fill it in from the request's `User-Agent` header instead.

`log_file`, when sent, SHOULD cover roughly the last day of that client's operational logs.

A client that attaches logs MUST warn the user what will be sent (that a day of logs is included) before submitting.

The server SHOULD reject a `log_file` above a reasonable size cap (e.g. 8MB) with **HTTP 400**.

The server SHOULD rate limit this endpoint by client IP address, since it MAY be called without authentication, and MUST respond **HTTP 429** with `{ "error": "Too many requests" }` once the limit is exceeded.

The server MUST email the report to a fixed internal address and include, the message, `platform`/`app_version`/`platform_details` if available, and, when authenticated, the reporting user's or device's identity.

The server SHOULD set the Reply-To header to the `contact_email` or the email of the authenticated account.

On success, the server MUST respond **HTTP 204**.
