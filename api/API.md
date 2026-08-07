# Virtue Initiative API Spec

Base URL examples:

- Local: `http://127.0.0.1:8787`
- Production: your deployed Workers URL

## Types

- `UUID`: UUIDv4 string
- `DateTime`: millisecond Unix timestamp
- `Base64`: base64-encoded binary
- `SHA256`: lowercase hex-encoded SHA-256 digest
- `RefreshToken`: opaque web-session string, prefixed by purpose (e.g. `wst_...`). Set as the HTTPOnly `refresh_token` cookie by `POST /login`, `POST /signup`, and `POST /email-verification/validate`.
- `HashServerToken`: EdDSA JWT (`Ed25519`) with `type: "hash-server"` and `sub = device id`; minted by `POST /d/device`, `GET /d/device`, and `POST /d/batch`
- `DeviceRefreshToken`: opaque string returned by `POST /d/device`
- `ServerToken`: EdDSA JWT (`Ed25519`) with `type: "server"` and `sub = device id`

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
  "settings": {
    "email_frequency": "none" | "alerts-only" | "daily" | "weekly",
    "timezone": "America/New_York"
  },
  "name": "Name" | undefined,
  "pub_key": Base64 | undefined,
  "encrypted_priv_key": Base64 | undefined
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
  "last_hash_at": DateTime | null,
  "pending_count": Number,
  "status": "online" | "offline"
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
  "encrypted_key": Base64,
  "created_at": DateTime
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

### `GET /`

Health/info endpoint. No auth.

Response `200`:

```js
{
  "name": "Virtue Initiative API",
  "version": "1.0.0",
  "status": "ok"
}
```

### `GET /.well-known/jwks.json`

Returns the public signing key in JWKS form for remote JWT verification.

Response `200`:

```js
{
  "keys": [
    {
      "kty": "OKP",
      "crv": "Ed25519",
      "x": "base64url-public-key",
      "alg": "EdDSA",
      "use": "sig",
      "kid": "thumbprint"
    }
  ]
}
```

### `GET /user/login-material?email=user@example.com`

Returns the current client password-derivation settings, and — when `email` is provided — a login
salt for that email. The response shape is the same for existing and non-existing users (a decoy
salt is returned for unknown emails, for enumeration resistance).

Response `200` (no `email`):

```js
{
  "params": HashParams
}
```

Response `200` (with `email`):

```js
{
  "password_salt": Base64,
  "params": HashParams
}
```

### `POST /signup-request`

Request:

```js
{
  "email": "user@example.com",
  "partner_invite_token": "opaque-string" | undefined
}
```

Response `200`:

```js
{ "ok": true }
```

Notes:

- Stores a short-lived pending signup token (`email_tokens` row with `purpose='signup'` and `user_id=NULL`).
- Sends a verification email with a link to `/finish-signup?token=...`.
- Does **not** create a user account.
- Never returns a distinct status for an already-registered email (enumeration-safe). If an
  account already exists, no token is issued; instead a notice email is sent to the existing
  account telling them someone tried to sign up with their address. The response is always
  `{ "ok": true }`.

### `POST /signup`

Request:

```js
{
  "verification_token": "opaque-string",
  "password_auth": Base64,
  "password_salt": Base64,
  "pub_key": Base64,
  "encrypted_priv_key": Base64,
  "name": "Name" | undefined,
  "email_digest_minutes_utc": Number | undefined,
  "partner_invite_token": "opaque-string" | undefined
}
```

Response `201`:

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

Notes:

- Looks up the pending signup record by `verification_token`; returns `400` if expired/not found.
- Creates the account using `email` from the pending record + crypto material from the body.
- Account is created with `email_verified = true`. Sets the `refresh_token` cookie (auto-login on signup).
- The signup token is consumed on success.
- If an account was created for this email in the window between `/signup-request` and `/signup`
  (a token-redemption race), the token is consumed and the same generic `400` used for
  expired/invalid tokens is returned — no distinct error, no new account created.
- `partner_invite_token` is accepted for forwarding to the client and should be applied by the client through `POST /partner/accept` after signup.

### `POST /login`

Request:

```js
{
  "email": "user@example.com",
  "password_auth": Base64,
  "timezone": "America/New_York" | undefined
}
```

Response `200`:

```js
{
  "ok": true
}
```

Also sets the `refresh_token` cookie (the only way this token is delivered). If the credentials are valid but the account email is
unverified, returns `403` with:

```js
{
  "error": "Please verify your email before logging in."
}
```

### `POST /email-verification/validate`

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
  "email": "user@example.com",
  "purpose": "email_change"
}
```

Notes:

- Applies a pending `email_change` token; returns `400` for any other or expired token.
- Also creates a web session and sets the `refresh_token` cookie (auto-login on verification).
- If the target email was claimed by another account in the window between `PATCH /user` and
  redemption (a race), the token is consumed, a notice is sent to the real owner, and the same
  generic `400` is returned — no distinct error, no session created.

### `POST /logout`

Clears the `refresh_token` cookie.

Response: `204 No Content`

### `GET /user`

Requires an authenticated web session (the `refresh_token` cookie, or `Bearer <RefreshToken>`).

Response `200`:

```js
User;
```

### `PATCH /user`

Requires an authenticated web session (the `refresh_token` cookie, or `Bearer <RefreshToken>`).

Request:

```js
{
  "email": "new@example.com" | undefined,
  "name": "New Name" | undefined,
  "email_frequency": "none" | "alerts-only" | "daily" | "weekly" | undefined,
  "pub_key": Base64 | undefined,
  "encrypted_priv_key": Base64 | undefined
}
```

Response `200`:

```js
{
  "ok": true,
  "email_verification_required": true | undefined,
  "pending_email": "new@example.com" | undefined
}
```

Notes:

- When `email` is changed, the user email is **not** updated immediately.
- A pending `email_change` token is sent to the new address.
- Submitting that token to `POST /email-verification/validate` applies the email update.
- If the requested email is already in use by another account, no distinct error is returned —
  the response looks identical to the success case (`email_verification_required: true`), but no
  working verification token is issued for the requester. Instead, a notice is sent to the actual
  owner of that email.
- Name/settings/key-material fields are applied unconditionally, regardless of whether the email
  change itself succeeds.

### `DELETE /user`

Requires an authenticated web session (the `refresh_token` cookie, or `Bearer <RefreshToken>`).

Permanently deletes the account; D1 rows owned by the account (devices, batches, sessions, email
tokens) cascade-delete along with it. `confirm_email` must match the account email or the request
returns `400`. R2 batch blobs are **not** deleted inline — they age out independently via a bucket
lifecycle rule, same as the general 30-day batch retention policy.

Request:

```js
{
  "confirm_email": "user@example.com"
}
```

Response: `204 No Content` (also clears the `refresh_token` cookie).

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
  "encrypted_priv_key": Base64
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

Requires an authenticated web session (the `refresh_token` cookie, or `Bearer <RefreshToken>`).

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

Requires an authenticated web session (the `refresh_token` cookie, or `Bearer <RefreshToken>`).

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

Requires an authenticated web session (the `refresh_token` cookie, or `Bearer <RefreshToken>`).

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

Requires an authenticated web session (the `refresh_token` cookie, or `Bearer <RefreshToken>`).

Request:

```js
{
  "digest_cadence": "none" | "alerts-only" | "daily" | "weekly" | undefined
}
```

Response: `204 No Content`

### `DELETE /partner/watcher/:id`

Requires an authenticated web session (the `refresh_token` cookie, or `Bearer <RefreshToken>`).

Response: `204 No Content`

### `DELETE /partner/watching/:id`

Requires an authenticated web session (the `refresh_token` cookie, or `Bearer <RefreshToken>`).

Response: `204 No Content`

## Devices

User-facing device management. These are distinct from the device-authenticated `/d/*` routes
below.

### `GET /device`

Requires an authenticated web session (the `refresh_token` cookie, or `Bearer <RefreshToken>`).

Lists the requester's own devices plus those of users they watch. `last_hash_at` and
`pending_count` reflect live hash-server state when a hash server is configured, otherwise the
last values stored in D1.

Response `200`:

```js
[Device];
```

### `PATCH /device/:id`

Requires an authenticated web session (the `refresh_token` cookie, or `Bearer <RefreshToken>`).

Renames a device the requester owns. Returns `404` if the device does not exist or is not owned by
the requester.

Request:

```js
{
  "name": "New Device Name"
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

Requires an authenticated web session (the `refresh_token` cookie, or `Bearer <RefreshToken>`).

Deletes a device the requester owns, along with its stored batches, and emails the owner and
accepted watchers. Returns `404` if the device does not exist or is not owned by the requester.

Response: `204 No Content`

## Data

### `GET /data`

Requires an authenticated web session (the `refresh_token` cookie, or `Bearer <RefreshToken>`).

Query parameters:

- `device_id`: optional `UUID`
- `user`: optional `UUID`
- `since`: optional timestamp, default `0`

Response `200`:

```js
{
  "batches": [BatchData]
}
```

`batches` only include rows where the requester has a matching `encrypted_key` envelope.
Returns every batch with `created_at > since`, ordered oldest-to-newest. Callers
should pass the largest `created_at` they've seen as `since` on subsequent syncs.

## Device API

The following routes use device auth:

- `POST /d/device` takes the owner's email + `password_auth` directly (same credential
  material as `POST /login`) — no web session or prior device session required
- `POST /d/logout`, `GET /d/device`, and `POST /d/batch` use a `DeviceRefreshToken`
- `POST /hash`, `GET /hash`, and `DELETE /hash` use a `HashServerToken` or `ServerToken` as applicable

### DeviceSettings

Shared shape embedded (alongside a fresh `token`) in the responses of `POST /d/device`,
`GET /d/device`, and `POST /d/batch`:

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
  "hash_base_url": "https://..." | null
}
```

`wrapping_keys` lists every public key the device must wrap batch keys for: the
device owner (when they have a public key) followed by all accepted partners.
The owner, when present, is always the first entry.

### `POST /d/device`

Registers a device for the user identified by `email`/`password_auth` — the same
credential verification `POST /login` performs (401 on bad credentials, 403 if the
account's email isn't verified). Since this is the device's very first call, no prior
session of any kind is required.

Request:

```js
{
  "email": "user@example.com",
  "password_auth": Base64,
  "name": "My Laptop",
  "platform": "linux"
}
```

Response `201`:

```js
{
  "refresh_token": DeviceRefreshToken,
  "settings": DeviceSettings,
  "token": HashServerToken
}
```

Registration alone is enough to start operating: the device gets its credentials,
initial settings, and an initial hash token in one round trip, with no follow-up call
needed before its first hash upload.

### `GET /d/device`

Refreshes settings and mints a fresh hash-server token for the authenticated device.
This is the manual/periodic refresh path — used by clients when their cached hash token
goes stale without a batch upload happening in the meantime — and also the replacement
for the removed `POST /d/token`.

Response `200`:

```js
{
  "settings": DeviceSettings,
  "token": HashServerToken
}
```

### `POST /d/logout`

Revokes the authenticated device's session and soft-deletes it. Clears the device's
hash-chain state as a best-effort cleanup; batches/screenshots and the device row itself
are untouched (that's the separate manual hard-delete flow via `DELETE /device/:id`).

Response: `204` with no body.

### `POST /d/batch`

Multipart form request:

- `start_time`: integer
- `end_time`: integer
- `access_keys`: JSON string
- `high_risk_count`: non-negative integer, optional, default `0`
- `medium_risk_count`: non-negative integer, optional, default `0`
- `notifications`: JSON string, optional — array of alert-email entries (see below)
- `file`: encrypted batch blob

`high_risk_count`/`medium_risk_count` are risk-band tallies computed client-side from the
per-event `risk` values in this batch (thresholds mirror `shared-web/risk.ts`). Since
event bodies are end-to-end encrypted, these counts are the only server-visible signal
used to summarize tamper activity in partner digest emails.

`access_keys` JSON shape:

```js
{
  "keys": {
    "<user_id UUID>": Base64 // hpke_key
  }
}
```

`notifications` JSON shape — a JSON-encoded array, each entry triggering the alert email
for one high-risk event once the batch is durably persisted (best-effort: a failure here
does not roll back the already-committed batch). The event body itself lives in the
encrypted batch; each entry only carries what the notification email needs:

```js
[
  {
    ts: DateTime,
    type: 'system_event',
    risk: 0.7,
    title: 'Device reported system event.' | undefined,
    details: '...' | undefined,
  },
];
```

`risk` is required (`0`-`1`) on each entry. `title`/`details` are optional; when omitted,
a default title is derived from `type` and no details are included.

Response `201`:

```js
{
  "id": UUID,
  "start_time": DateTime,
  "end_time": DateTime,
  "end_hash": SHA256,
  "url": "https://.../user/.../batches/...enc",
  "settings": DeviceSettings,
  "token": HashServerToken
}
```

Every batch upload refreshes both settings and the hash token, piggybacking on the
response so the device rarely needs a dedicated `GET /d/device` call.

## Hash API

### `POST /hash`

Requires a `HashServerToken`.

Uploads a single 32-byte plaintext content hash for the device hash chain.

Response `200`:

```js
{
  "ok": true
}
```

### `GET /hash`

Requires a `HashServerToken`.

Returns the current 32-byte hash-chain state as binary.

### `GET /hash/info`

Requires a `HashServerToken` or `ServerToken`.

Returns hash-chain metadata for the device.

Response `200`:

```js
{
  "count": Number,
  "hashed_at": DateTime | null,
  "updated_at": DateTime | null
}
```

### `DELETE /hash`

Requires a `ServerToken`.

Resets the current device hash-chain state.

## Object storage

### `GET /r2/*`

Serves an object from the R2 bucket by key (the path after `/r2/`). This is where the
`url` on a `BatchData` points, so clients fetch encrypted batch blobs here. No auth: blobs are
end-to-end encrypted and only decryptable by recipients holding the wrapped batch key.

Response `200`: the raw object body with its stored content headers and `etag`. Returns `404` if
the key is missing or not found.

## Webhooks

### `POST /email/sns`

Inbound AWS SNS webhook for SES delivery events; not called by clients. Confirms the SNS
subscription (`SubscriptionConfirmation`) and processes `Bounce`/`Complaint` notifications by
marking the affected users' emails as bounced and unverified.

Response `200`:

```js
{ "ok": true, "subscribed": true }   // SubscriptionConfirmation
{ "ok": true, "updated": Number }    // Bounce/Complaint notification
{ "ok": true }                        // ignored event
```
