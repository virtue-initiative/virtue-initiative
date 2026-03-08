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

To expose the dev server to VMs on your local network (for example libvirt guests):

```bash
npm run dev -- --ip 0.0.0.0 --port 8787
```

From a libvirt VM on the default network, this is typically reachable at:

- `http://<HOST_IP>:8787` (replace `<HOST_IP>` with your Linux host IP reachable from the VM)

## Deployment

Ensure all the enviroment variables are set correctly (in `wrangler.toml` and
using `wrangler secret put`) and run

```bash
npm run deploy
```

## API

See the [API specification](./API.md).
