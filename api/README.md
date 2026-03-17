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

Set up the environment:

```bash
npm install
npm run db:migrate
cp .dev.vars.example .dev.vars
```

Run the development server

```bash
npm run dev
```

The API will be available at `http://localhost:8787`

`npm run dev` uses the `staging` environment config, but Wrangler serves D1 and
R2 locally in development so it does not touch the real staging resources.

With `API_BASE_PATH=/api`, the same worker will also accept `http://localhost:8787/api/*`
and strip that prefix before routing. That matches production setups where both
`api.virtueinitiative.org/*` and `virtueinitiative.org/api/*` point to the same worker.

To expose the dev server to VMs on your local network (for example libvirt guests):

```bash
npm run dev -- --ip 0.0.0.0 --port 8787
```

From a libvirt VM on the default network, this is typically reachable at:

- `http://<HOST_IP>:8787` (replace `<HOST_IP>` with your Linux host IP reachable from the VM)

When serving local clients from emulators/VMs (for example Android using `10.0.2.2`),
set this in `api/.dev.vars` so `/d/batch` internal hash calls stay host-local:

```bash
HASH_SERVER_URL=http://127.0.0.1:8787
```

## Deployment

Ensure all the enviroment variables are set correctly (in `wrangler.json` and
using `wrangler secret put`) and run

```bash
npm run deploy:prod
```

Cloudflare still needs both hostnames/routes pointed at this worker. `API_BASE_PATH`
only makes the `/api` prefix optional once the request reaches the worker; it does not
create the Cloudflare route itself.

`APP_URL` is the single source for both outbound web links and the allowed CORS origin.

If you manage Cloudflare Transform Rules outside this repo, a zone-level URL Rewrite Rule
that rewrites `/api/*` to `/*` can replace this Worker-side prefix handling.

## API

See the [API specification](./API.md).
