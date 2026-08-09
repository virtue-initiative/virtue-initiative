# hash-server

Standalone Axum + sqlx (SQLite) Rust server that verifies EdDSA JWTs and
maintains a per-device SHA-256 hash chain. It's plain HTTP only — no TLS
layer — and isn't deployed anywhere today; in prod/staging, `HASH_SERVER_URL`
points at the Cloudflare Worker's own D1-backed reimplementation
(`api/src/routes/hashes.ts`) instead.

## Endpoints

| Method & path  | Auth                                          | Purpose                                                                                                        |
| -------------- | --------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `POST /hash`   | `device-cert` JWT + Ed25519 request signature | Extend the device's hash chain with a new 32-byte hash                                                         |
| `GET /hash`    | `device-access` or `server` JWT               | Get `{ state, count, hashed_at, updated_at }` for the device's chain (merged with the former `GET /hash/info`) |
| `DELETE /hash` | `server` JWT                                  | Reset the device's chain state to all zeros                                                                    |
| `GET /health`  | none                                          | Liveness check                                                                                                 |

Auth is a Bearer JWT (EdDSA), verified against `JWT_PUBLIC_KEY`. The token's
`sub` claim is the device ID; the `type` claim determines which endpoints it
can call.

`POST /hash` is the one per-device, high-frequency, TLS-handshake-sensitive
path — the reason this server runs over plain HTTP instead of TLS in the
first place (see "Performance testing" below). In place of TLS, every
`POST /hash` request carries a `device-cert`-typed JWT (embedding the
device's Ed25519 pubkey, minted by the API Worker's `buildDeviceState` in
remote-hash-server mode — never persisted here) plus a per-request Ed25519
signature over `X-Signature-Timestamp`/`X-Signature` headers. The server
rejects stale (`|now - timestamp| > 60s`) or replayed/non-increasing
timestamps, tracked per device in an in-memory watermark
(`AppState.replay_guard`, reset on restart). See `src/auth.rs`'s
`verify_signature` and the root `CLAUDE.md`'s "Device-cert request signing"
contract for the exact signed-message byte layout. `GET /hash` and
`DELETE /hash` are server-only (only ever called by the API Worker, never a
device directly) and stay on the old unsigned bearer-JWT scheme.

Hash chain state is stored in a single SQLite table (`hash_states`), keyed by
`device_id`.

## Running locally

Copy `.env.example` to `.env` and fill in `JWT_PUBLIC_KEY` (an EdDSA public
key PEM):

```
JWT_PUBLIC_KEY="-----BEGIN PUBLIC KEY-----\n...\n-----END PUBLIC KEY-----"
DATABASE_URL=sqlite:hash-states.db
PORT=3000
ALLOWED_ORIGINS=http://localhost:5173
```

Then:

```sh
cargo run
```

This applies migrations automatically and starts listening on `PORT`
(default `3000`).

## Running tests

```sh
cargo test
```

Runs the inline `#[cfg(test)]` correctness tests in `src/routes.rs` and
`src/db.rs` against an in-memory SQLite database.

## Performance testing

`examples/loadtest.rs` is a `cargo`-native load generator that hits a
running `hash-server` instance with traffic modeled on the real production
pattern: per-device `POST /hash` pings (plain HTTP, signed with a per-device
Ed25519 identity key and a `device-cert` JWT — see "Endpoints" above),
per-device `DELETE /hash` resets, and per-user-group `GET /hash` info bursts
(both of the latter over HTTPS with the old unsigned `server`-token scheme,
since neither ever comes from a real device — see `--secure-url` below). See
the doc comment at the top of the file for the traced-through cadence and
the per-device memory model. It's a manually-run local tool, not wired into
CI.

Each simulated device gets its own deterministic Ed25519 identity keypair
(seeded from its index, distinct from the fixed seed reserved for the
simulated server's own JWT-signing key below) so `POST /hash` requests are
signed the same way a real device would sign them.

### 1. Set up a matching signing key

The load test mints JWTs using a fixed, deterministic Ed25519 seed (for
reproducibility — minting isn't part of what's measured). Print the matching
public key and set it as `JWT_PUBLIC_KEY` in `.env`:

```sh
cargo run --release --example loadtest -- --duration-secs 0
```

The tool prints a PEM block like:

```
-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEA...
-----END PUBLIC KEY-----
```

Paste that into `hash-server/.env` as `JWT_PUBLIC_KEY`.

### 2. Start the server

Use a release build so perf numbers reflect an optimized binary, not debug:

```sh
cargo run --release
```

### 3. Run the load test

In another terminal:

```sh
cargo run --release --example loadtest -- --url http://localhost:3000 --duration-secs 60
```

It prints a per-endpoint summary: total requests, error count (non-2xx or
connection failures), throughput (req/s), and min/p50/p95/p99/max latency,
broken out by `POST /hash`, `DELETE /hash`, and `GET /hash` (info).

### CLI flags

| Flag                           | Default                                    | Meaning                                                                                                 |
| ------------------------------ | ------------------------------------------ | ------------------------------------------------------------------------------------------------------- |
| `--url`                        | `http://localhost:3000`                    | Base URL for per-device traffic (`POST /hash`) — plain HTTP                                             |
| `--secure-url`                 | `--url` with its scheme swapped to `https` | Base URL for server-only traffic (`DELETE /hash`, the merged `GET /hash` info burst) — TLS-fronted      |
| `--devices-per-user`           | `2`                                        | Devices per simulated user (real fleets are ~2-3); sizes each INFO-burst group                          |
| `--users`                      | `250`                                      | Number of independent simulated users/fleets (500 devices total by default)                             |
| `--post-interval-secs`         | `300`                                      | Real-world seconds between a device's `POST /hash` pings, before `--time-scale`                         |
| `--reset-interval-secs`        | `3600`                                     | Real-world seconds between a device's `DELETE /hash` resets, before `--time-scale`                      |
| `--info-session-interval-secs` | `1800`                                     | Real-world seconds between a user's `GET /hash` session bursts, before `--time-scale`                   |
| `--time-scale`                 | `60`                                       | Divides all three intervals above by this factor, to compress cadence into a short test run             |
| `--duration-secs`              | `120`                                      | How long to run                                                                                         |
| `--workers`                    | `256`                                      | Size of the fixed worker-task pool that signs and sends per-device `POST`/`DELETE` requests (see below) |

With the defaults, `--time-scale 60` turns the real cadence (POST/5min,
DELETE/hour, INFO/30min) into POST every 5s, DELETE every 60s, and an INFO
burst every 30s per user. Use `--time-scale 1` for a literal-cadence,
multi-hour soak run.

### Notes

- **Per-device memory model.** Per-device traffic (`POST`/`DELETE`) is not
  one `tokio` task per device — at 500k simulated devices, that model's
  per-task overhead measured out to roughly 7.9GB, which made ramping toward
  a 1M-device target impractical. Instead every device's state lives as one
  entry in a flat `Arc<[DeviceState]>`, scanned once per tick by a single
  scheduler task that enqueues due actions onto a channel drained by the
  fixed `--workers` pool. See the module doc comment at the top of
  `examples/loadtest.rs` for the full design (including how it avoids
  redundantly re-enqueuing a device whose request is still in flight).
- **Replay-guard memory at scale.** `hash-server` itself keeps an in-memory
  `AppState.replay_guard: DashMap<String, i64>` mapping each device that has
  ever sent a signed `POST /hash` to its last-accepted signature timestamp.
  At 1M devices (UUID-length `String` key plus `i64` value plus `DashMap`'s
  per-entry/shard overhead) this is roughly 100-150MB — small relative to
  the load generator's own per-device state, but worth knowing about before
  ramping the device count on constrained hardware.
- The server opens its write transactions with `BEGIN IMMEDIATE` (not a bare
  `BEGIN`) and runs with `PRAGMA synchronous = NORMAL` under WAL — see
  `src/db.rs`'s `update_hash_chain` and `src/main.rs`'s `SqliteConnectOptions`
  setup. Without these, concurrent `POST`/`DELETE` load hits spurious
  `SQLITE_BUSY` ("database is locked") errors on transaction upgrade and pays
  a `fsync` per commit; with them, the same load (2500 simulated devices)
  sustains ~2500 req/s on `POST /hash` with p99 latency around 50ms instead
  of a ~70% error rate and multi-second p99s.
- SQLite is still a single writer, so extreme sustained write concurrency
  will eventually queue on the write lock — if you push `--users`/
  `--time-scale` far enough to saturate it, `busy_timeout` (30s) governs how
  long a write waits before giving up.
