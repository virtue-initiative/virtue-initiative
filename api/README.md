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

## Deployment

Ensure all the enviroment variables are set correctly (in `wrangler.toml` and
using `wrangler secret put`) and run

```bash
npm run deploy
```

## API

See the [API specification](./API.md).
