# Hash server

A small Rust HTTP server that keeps a running hash of every event each device records. Devices
send each event's hash here as they record it, and the API asks for the running hash when a
batch is uploaded, so a batch can't be edited after the fact without the web app noticing
([`docs/overview.md`](../docs/overview.md), BATCH-006). Its behavior, implementation choices and
deployment are specified in [`SPEC.md`](SPEC.md) (`HASH-` IDs).

## Run it

`./scripts/launch.sh` starts it alongside the API, with the API's public key, so you rarely run
it by hand. To run it alone, from `hash-server/`:

```bash
JWT_PUBLIC_KEY="$(cat path/to/jwt-public.pem)" cargo run
```

It listens on port 8788 and keeps its SQLite database in `hash-server.sqlite`. `HOST`, `PORT`,
`DATABASE_PATH` and `WRITE_BATCH_WINDOW_MS` change that (HASH-012); a `.env` file is read if
present.

## Test

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

`just mutants-hash` runs mutation testing (see `TESTING.md`). `scripts/bench.sh` measures
throughput with `h2load` (HASH-016); `cargo run --example mint_token` makes the tokens it needs.

## Deploy

`.github/workflows/deploy-hash-server.yml` builds the binary and copies it with its systemd unit
(`deploy/`) to the server over SSH: pushes to `staging` deploy `staging-virtue-hash`, pushes to
`main` deploy `virtue-hash` (HASH-018). `deploy/env.example` lists the environment each needs.
