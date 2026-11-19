# Web app

The web app at https://app.virtueinitiative.org: sign-up, partners, devices, and reading logs
and reports. Every batch is decrypted in the browser. What it does is specified in
[`SPEC.md`](SPEC.md) (`WEB-` IDs); [`AGENTS.md`](AGENTS.md) lists the rules that are easy to
break.

## Run it

Usually you run it with the rest of the stack: `./scripts/launch.sh` from the repo root (see
[`docs/development.md`](../docs/development.md)). To run only the web app against an API you
already have:

```bash
cp .env.example .env.local   # then set VITE_API_URL
just web-dev
```

To open the local web app from a phone or another machine over HTTPS, see
[HTTPS-DEVICE-TESTING.md](HTTPS-DEVICE-TESTING.md).

## Check it

```bash
just web-typecheck
just web-test
just web-build
```

`bun run format:check` checks formatting. `just mutants-web` runs mutation testing on the
crypto and cache code (see `TESTING.md`).

## Deploy

CI deploys on pushes to `staging` and `main` (`.github/workflows/deploy.yml`). By hand:
`bun run deploy:staging` or `bun run deploy:prod`.

## Stack

Preact, TypeScript, Vite, plain CSS with the tokens from `shared-web/`, and
[Heroicons](https://heroicons.com/). The local cache is SQLite in OPFS, run in a worker.
