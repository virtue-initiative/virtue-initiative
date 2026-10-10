# Batch Format

## BATCH-001 Overview

A batch is an end-to-end encrypted group of events from one device. The device (producer) builds it, the API stores it without being able to read it, and the web app (consumer) decrypts and verifies it.

A batch has three parts:

- the **blob**: the encrypted events (BATCH-002)
- the **access keys**: the batch key wrapped once per recipient (BATCH-005)
- the **end hash**: the hash chain state used to verify the events (BATCH-006)

This spec defines the bytes. How a batch is uploaded and served is defined by API-038 and API-033.

```
event   = msgpack({ts, risk?, type, data?})
payload = msgpack([event, ...])
blob    = nonce[12] || AES-256-GCM(batch_key, nonce, gzip(payload))
```

## BATCH-002 Blob

The producer MUST build the blob as follows:

1. Encode each event to bytes (BATCH-003).
2. Encode the list of encoded events as one msgpack array (BATCH-004).
3. Compress the result with gzip.
4. Encrypt the result with AES-256-GCM, using a fresh random 32-byte batch key, a fresh random 12-byte nonce, and no additional authenticated data.
5. Output `nonce || ciphertext || tag`, where `tag` is the 16-byte GCM tag.

A batch key MUST NOT be reused for another batch. A batch MUST contain at least one event. The producer SHOULD limit a batch to 200 events (BATCH-006).

The consumer MUST treat any failure to decrypt, decompress or decode the blob as permanent, and MUST NOT show any events from that batch.

## BATCH-003 Event

An event MUST be encoded as a msgpack map with string keys:

| Key    | Type    | Required | Meaning                                                              |
| ------ | ------- | -------- | -------------------------------------------------------------------- |
| `ts`   | integer | yes      | When the event happened, in milliseconds since the Unix epoch (UTC). |
| `risk` | float   | no       | Risk score from `0.0` to `1.0`. Absent means not scored.             |
| `type` | string  | yes      | Event type, see below.                                               |
| `data` | map     | no       | Fields specific to the type. Absent when the type has none.          |

The encoded bytes of an event are what the hash chain covers (BATCH-006). The producer MUST encode an event once and use the same bytes for both the hash and the batch. The consumer MUST hash the bytes as received and MUST NOT re-encode an event before hashing it.

Known types and their `data` fields:

| `type`               | `data`                                                                                     |
| -------------------- | ------------------------------------------------------------------------------------------ |
| `screenshot`         | `image`: bytes, `content_type`: string, `skin_detection`?: float, `nsfw_detection`?: float |
| `screenshot_skipped` | `reason`: `"static_screen"` or `"locked_or_screensaver"`                                   |
| `screenshot_missed`  | none                                                                                       |
| `capture_failed`     | none                                                                                       |
| `heartbeat`          | none                                                                                       |
| `user_stop`          | none                                                                                       |
| `user_start`         | none                                                                                       |
| `system_login`       | `utc_ms`: integer                                                                          |
| `system_logout`      | `utc_ms`: integer                                                                          |
| `repeated_restarts`  | `count`: integer, `window_ms`: integer                                                     |
| `alert`              | `message`: string                                                                          |
| `dev`                | `title`: string, `details`: string or nil                                                  |

The consumer MUST ignore keys it does not know and MUST NOT reject an event because its `type` is unknown. A new type or a new optional field is therefore not a breaking change.

## BATCH-004 Byte strings

A byte string appears in two places: each element of the payload array (an encoded event), and the `image` field of a `screenshot` event.

The producer MUST encode a byte string as a msgpack `bin` value.

The consumer MUST also accept a msgpack array of integers from 0 to 255, because batches written before this requirement use that form.

## BATCH-005 Access keys

The producer MUST wrap the batch key once for each recipient, using HPKE (RFC 9180) in base mode with `DHKEM(X25519, HKDF-SHA256)`, `HKDF-SHA256` and `AES-256-GCM`. The `info` and `aad` inputs MUST be empty.

The wrapped key is `enc || ciphertext`, where `enc` is the 32-byte encapsulated key and `ciphertext` is the sealed 32-byte batch key with its tag (80 bytes in total). It MUST be sent as standard padded Base64.

The recipients are the device's `wrapping_keys` (see `DeviceSettings` in API-002): the owner and every accepted partner. The producer MUST NOT build a batch with no recipients. The wrapped keys are sent as `access_keys`, a map from recipient user ID to wrapped key (API-038).

The consumer receives only its own wrapped key, as `encrypted_key` (see `BatchData` in API-002), and MUST open it with the user's private key to recover the batch key.

## BATCH-006 Hash chain

The hash chain lets a consumer detect events that were added to, removed from, or changed in a batch after the device recorded them.

For each event, in order, the producer MUST send `content_hash` to the hash server (HASH-006):

```
content_hash = sha256(event_bytes)
state        = sha256(state || content_hash)
```

The state starts at 32 zero bytes. When a batch is uploaded, the API records the hash server's current state as the batch's `end_hash` and resets the state to zero. Each batch is therefore verified on its own.

The producer MUST put events in the batch in the same order their hashes were sent. A batch MUST contain exactly the events whose hashes the hash server has accepted since the previous batch, because `end_hash` covers all of them. To limit the size of a batch, the producer MUST stop sending hashes once the limit is reached, until that batch is uploaded.

To verify a batch, the consumer MUST start from 32 zero bytes, apply the two steps above to each event in payload order, and compare the final state to `end_hash`. If they differ, the consumer MUST mark every event in the batch as failing verification. It SHOULD still show them.

## BATCH-007 Versioning

The API stores a `version` with each batch (API-038) and returns it in `BatchData`. This spec describes version `v0.1`.

A change that an existing consumer cannot read, or that changes the bytes covered by the hash chain, is a breaking change. It MUST come with a new version.
