# Overall Core Architecture

This folder contains a Rust core implementation for Virtue screenshot monitoring.

The main design rule is:

- platform crates capture raw screen data
- `core` owns request flow, persistence, retrying, hashing, batch construction, compression, encryption, and upload semantics

## Workspace Layout

```text
client/
  core/
    architecture.md
    Cargo.toml
    src/
      api.rs
      batch.rs
      config.rs
      crypto.rs
      error.rs
      lib.rs
      model.rs
      platform.rs
      service.rs
      storage.rs
  linux/
    ...
```

## Public Surface

`core` exposes `MonitorService<P>` where `P: PlatformHooks`.

Public methods:

```rust
MonitorService::setup(config, platform) -> Result<Self>
MonitorService::loop_iteration() -> Result<LoopOutcome>
MonitorService::shutdown() -> Result<()>
MonitorService::send_log(log) -> Result<()>
MonitorService::login(username, password) -> Result<LoginStatus>
MonitorService::logout() -> Result<()>
MonitorService::status() -> Result<ServiceStatus>
```

`PlatformHooks` stays intentionally small:

```rust
take_screenshot() -> Result<Screenshot>
get_time_utc_ms() -> Result<i64>
```

Everything else belongs to `core`.

## Core State Model

`core` persists its own restart-safe state under `Config.state_dir`.

Current files:

- `status.json`: last known runtime status
- `auth.json`: user id, user token, stored wrapping key, and device credentials
- `device_settings.json`: latest device settings returned by `GET /d/device`
- `pending_requests.json`: retry queue
- `batch_buffer.json`: buffered plaintext screenshot events waiting for batch upload
- `errors.log`: local append-only operational error log

`core` should be able to restart and continue uploading without platform-specific recovery logic.

## Device/Auth Model

The device-side runtime uses:

- user access token from `POST /login`
- device id, device access token, and device refresh token from `POST /d/device`
- device settings from `GET /d/device`

The service primarily depends on device credentials after login. User refresh-cookie handling is not a core runtime dependency for background upload.

The device settings response is important because it provides:

- `enabled`
- encrypted `e2ee_key`
- `hash_base_url`

The actual batch encryption key is restored when device settings are fetched.

Instead, login restores it using the same flow as the web app:

1. log in with the argon2id-hashed password
2. parse `userId` from the returned access token `sub`
3. derive a wrapping key with PBKDF2-HMAC-SHA256 using:
   - password = original plaintext password
   - salt = UTF-8 user id
   - iterations = 100,000
4. persist that wrapping key locally
5. call `GET /d/device`
6. decrypt `device_settings.e2ee_key` as `nonce[12] || ciphertext+tag` using the stored wrapping key
7. keep the decrypted 32-byte batch encryption key in runtime memory for whole-batch encryption

On restart, the client restores the stored wrapping key from `auth.json` and decrypts the cached or freshly fetched `device_settings.e2ee_key` again.

## Screenshot Model

Each screenshot event remains plaintext inside the batch payload.

That means:

- individual screenshots are not encrypted one by one
- the `image` bytes inside each event are raw batch contents
- only the final batch blob is encrypted before upload

The intended screenshot event shape is:

```text
{
  ts: <ms epoch>,
  type: "screenshot",
  data: {
    image: <bytes>,
    content_type: "image/webp"
  }
}
```

The web app already expects decrypted batch events to contain screenshot bytes this way.

Before the event is built, the raw captured frame is processed in `core`:

1. decode source bytes
2. apply a light blur
3. resize so the smaller dimension is 128 px
4. encode as low-quality WebP

That keeps the platform boundary simple while preserving the older client behavior of aggressively reducing image detail and size before batching.

## Batch Blob Format

The batch blob format must match the existing web app.

### Plain batch payload

The plaintext batch payload is:

1. a MessagePack object
2. containing `events: [...]`
3. where each event includes plaintext screenshot bytes

Example logical structure:

```text
{
  events: [
    {
      ts: 1710000000000,
      type: "screenshot",
      data: {
        image: <bytes>,
        content_type: "image/webp"
      }
    }
  ]
}
```

### Compression

Before encryption, the MessagePack payload is gzip-compressed.

Pipeline:

```text
events -> msgpack -> gzip
```

### Encryption

After compression, the whole blob is encrypted with AES-256-GCM using the device `e2ee_key`.

Serialized wire format:

```text
nonce[12 bytes] || ciphertext_plus_tag
```

This matches the web app `encryptData()` / `decryptBatch()` format.

### Upload

That final encrypted blob becomes the `file` field for `POST /d/batch`.

## Hash Chain Model

Hash uploads are not hashes of encrypted batches.

They are hashes of individual plaintext events.

The event content hash must match the web app verification logic:

```text
content_hash = sha256(
  ts_le_64 ||
  type_utf8 ||
  sorted(data entries as key_utf8 || encoded_value)
)
```

Value encoding rules:

- strings: UTF-8 bytes
- numbers: 8-byte little-endian integer form
- booleans: one byte `0` or `1`
- byte arrays: raw bytes

The rolling hash update sent to the hash service is:

```text
new_state = sha256(current_state || content_hash)
```

The client does not compute server state itself, but it must produce compatible per-event `content_hash` bytes for `POST /hash`.

## Loop Semantics

`loop_iteration()` should:

1. get current UTC time
2. write/update current status
3. retry queued failed requests
4. if screenshot interval elapsed:
   - capture screenshot through platform hooks
   - normalize it into a batch event
   - compute its plaintext content hash
   - append event to `batch_buffer.json`
   - attempt `POST /hash`
5. if batch interval elapsed and buffered events exist:
   - build batch payload
   - MessagePack encode
   - gzip compress
   - AES-GCM encrypt
   - attempt `POST /d/batch`
6. persist all updated state
7. return the next due time

## Retry Model

All retry behavior lives in `core`.

Queued request record fields:

- request id
- request kind
- serialized payload
- last tried timestamp
- try count

When a request fails:

- queue it to disk
- retry it in future loop iterations

Exception:

- `400 Bad Request` errors are treated as permanent client-side failures
- they are written to `errors.log`
- they are dropped instead of retried

For device-authenticated endpoints:

- if the request returns `401`
- use `POST /d/token` with the stored device refresh token
- persist the new device access token
- retry the original request once

Request kinds that should support replay:

- upload hash
- upload batch
- upload log
- fetch device settings

## API Mapping

The core API client should implement:

- `POST /login`
- `POST /logout`
- `POST /d/device`
- `GET /d/device`
- `POST /d/token`
- `POST /d/batch`
- `POST /d/log`
- `POST /hash`

Additional important details from the existing system:

- login password must be argon2id-hashed before sending, matching the web app
- wrapping key derivation must mirror the web app PBKDF2 flow from plaintext password + user id
- encrypted batch key unwrap happens from `GET /d/device`, not `/user`
- multipart upload must send `file`, `start_time`, and `end_time`
- `POST /hash` sends exactly 32 bytes with `application/octet-stream`
- hash uploads may target `device_settings.hash_base_url` when present

## Implementation Notes

### Password hashing

Login must mirror the web app:

- Argon2id
- salt = lowercased email
- iterations = 3
- memory = 65536
- parallelism = 1
- output length = 32 bytes
- send lowercase hex digest as the password field

### MessagePack compatibility

The decrypted payload is consumed by `@msgpack/msgpack` in the web app.

The simplest compatible shape is:

```text
{
  events: [
    {
      ts: number,
      type: string,
      data: {
        image: binary,
        content_type: string
      }
    }
  ]
}
```

### Compression compatibility

The web app uses `DecompressionStream("gzip")`, so the Rust client must emit standard gzip data.

### Encryption compatibility

The web app expects:

- first 12 bytes = AES-GCM nonce
- remainder = ciphertext plus authentication tag

### Service shutdown

`shutdown()` should:

- attempt to upload a shutdown log
- persist state
- mark status as no longer running

