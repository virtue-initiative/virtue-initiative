# Virtue Initiative API

A screenshot accountability system API built on Cloudflare Workers.

## Technology Stack

- **Runtime**: [Cloudflare Workers](https://workers.cloudflare.com/)
- **Framework**: [Hono](https://hono.dev/)
- **Language**: TypeScript
- **Database**: [Cloudflare D1](https://developers.cloudflare.com/d1/)
- **Storage**: [Cloudflare R2](https://developers.cloudflare.com/r2/)
- **Testing**: Vitest with Cloudflare Workers pool

## Development

`./scripts/launch.sh` from the repo root runs the API with the web app, landing site and hash
server, and `./scripts/setup.sh` has already copied `.dev.vars` and run the local migrations
(see [`docs/development.md`](../docs/development.md)). To run only the API, from `api/`:

```bash
cp .dev.vars.example .dev.vars   # once
bun run db:migrate:local          # after pulling new migrations
just api-dev                      # http://localhost:8787
```

`bun run dev` uses the `staging` environment config, but Wrangler serves D1 and R2 locally, so it
does not touch the real staging resources.

With `API_BASE_PATH=/api`, the same worker also accepts `http://localhost:8787/api/*` and strips
that prefix before routing. That matches production, where both `api.virtueinitiative.org/*`
and `virtueinitiative.org/api/*` point to the same worker.

To reach the dev server from a VM or emulator on your network, listen on all interfaces:

```bash
bun run dev -- --ip 0.0.0.0 --port 8787
```

From a libvirt VM on the default network it is then at `http://<HOST_IP>:8787`, and from the
Android emulator at `http://10.0.2.2:8787`. When serving clients that way, set
`HASH_SERVER_URL=http://127.0.0.1:<hash port>` in `api/.dev.vars` so the API's own calls to the
hash server stay on the host.

Checks: `just api-typecheck`, `just api-test` (see [`TESTING.md`](TESTING.md)), and
`bun run format:check`. [`AGENTS.md`](AGENTS.md) lists the conventions that are easy to break.

## Deployment

Generate a signing keypair once:

```bash
openssl genpkey -algorithm ed25519 -out jwt-private.pem
openssl pkey -in jwt-private.pem -pubout -out jwt-public.pem
```

Set `JWT_PRIVATE_KEY` and `JWT_PUBLIC_KEY` in local `.dev.vars`, and in deployed
environments upload at least the private key with `wrangler secret put`. The
public key can be shared with any other server that needs to verify tokens, such
as the hash server.

Upload `LOGIN_SALT_SECRET` to each deployed environment before deploying, using
a long random value such as `openssl rand -base64 32`. Rotate it only after a
leak, because rotating it shows anyone who saved salts which ones were real.

Ensure all the environment variables are set correctly (in `wrangler.json` and
using `wrangler secret put`) and run

```bash
bun run deploy:prod
```

CI does this on pushes to `staging` and `main`. Neither CI nor the deploy scripts apply D1
migrations: run `bun run db:migrate:staging` or `bun run db:migrate:prod` before a deploy that
needs them.

Cloudflare still needs both hostnames/routes pointed at this worker. `API_BASE_PATH`
only makes the `/api` prefix optional once the request reaches the worker; it does not
create the Cloudflare route itself.

`APP_URL` is the single source for both outbound web links and the allowed CORS origin.

If you manage Cloudflare Transform Rules outside this repo, a zone-level URL Rewrite Rule
that rewrites `/api/*` to `/*` can replace this Worker-side prefix handling.

## API

See the [API specification](./SPEC.md).
