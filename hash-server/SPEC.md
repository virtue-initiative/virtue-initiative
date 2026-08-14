# Hash API Server

## 1. Overview

The hash server SHOULD have one endpoint with three methods.

### 1.1 Error Responses

All error responses (not **2xx**) MUST have this shape.

```json
{
  "code": "invalid_body",
  "message": "The request contains an invalid body",
  "details": "optional more details"
}
```

## 2. Methods

### 2.1 `POST /hash`

The client MUST authenticate with this header.

```
Authorization: Bearer <JWT>`
```

The JWT SHOULD be signed by the main API server and MUST have 'hash-server' as the type claim and the device ID as the sub.

The server MUST reject invalid JWTs with **HTTP 401**.

Client MUST send a 40 byte body, all integer fields little-endian.

```
[unix_time:u32][seq:u32][sha hash:32 bytes]
```

- `unix_time`: REQUIRED, ignored for now (not used for replay prevention).
- `seq`: REQUIRED, MUST be strictly greater than the last sequence number for the device, until it resets on DELETE.
- `sha hash`: REQUIRED, hash to be combined with the currently stored hash (see below).

The server MUST respond with **HTTP 400** if the body is invalid.

The server MUST respond with **HTTP 409** if the sequence number is not strictly greater than the previous sequence number.

**Hash Storage**

The server MUST take the hash and combine it with the stored hash in this way

```
stored = sha256(stored || hash)
```

The server MUST return **HTTP 201** if the new state has been written to disk and MUST NOT return **HTTP 201** otherwise.

The client SHOULD retry the request if it does not receive any of the following errors: `400`, `401`, `409`

### 2.2 `GET /hash?devices=[device_ids]`

The client (in this case, the main API server) MUST authenticate with this header.

```
Authorization: Bearer <JWT>
```

The JWT SHOULD be signed by the main API server and MUST have 'server' as the type claim. The sub claim SHOULD be ignored for `server` type tokens.

The server MUST respond with **HTTP 401**, if the JWT is invalid.

**device_ids** MUST be a comma seperate list of valid IDs.

The server SHOULD reject malformed IDs or a malformed list with **HTTP 400**.

On a valid request, the server MUST return the following JSON shape with **HTTP 200**, with one entry per device ID.

The server MUST return a ZERO hash, a 0 count and a 0 for last_received if it does not know of the device ID.

```json
{
  "device_id": {
     "hash": "hash_hex",
     "seq": 40,
     "last_received": 1786674101
  },
  ...
}
```

### 2.3 `DELETE /hash?device=device1`

The client MUST authenticate with this header.

```
Authorization: Bearer <JWT>
```

The JWT SHOULD be signed by the main API server and MUST have 'hash-server' as the type claim and the device ID as the sub.

The server MUST reject invalid JWTs with **HTTP 401**.

The server SHOULD return **HTTP 400** on a malformed `device_id`

The server MUST reset a device's hash to ZERO and also set the sequence number to zero.

On success, the server MUST return **HTTP 200** with the following shape. With the data, BEFORE it was reset.

```json
{
  "hash": "hash_hex",
  "seq": 40,
  "last_received": 1786674101
}
```

## 3. Implementation

### 3.1 High level overview

The server...

- SHOULD be implemented in rust.
- SHOULD serve strictly HTTP.
- SHOULD be behind a cloudflare tunnel which handles HTTPS.
- SHOULD use SQLite as its backend database
- SHOULD be as fast as possible

### 3.2 Server

The server SHOULD use tokio as it's runtime

### 3.3 Database

SQLite SHOULD be configured in WAL with synchronous = full.

Writes MUST all be on one thread and writes within a configurable time window SHOULD be batched as one transaction, with no maximum batch size.

Writes MUST be fully written to the database before a successful response is returned to the client.

## 4. Performance Testing

We SHOULD have a script that uses h2load to test the number of valid requests per second over http.
