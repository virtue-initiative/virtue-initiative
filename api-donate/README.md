# api-donate

Standalone Cloudflare Worker that powers the donations flow on the landing site. It creates
Stripe Checkout sessions and records completed donations in its own D1 database. It is intentionally
separate from the main `api/` worker and has no shared auth or data.

## Endpoints

- `GET /` — health check.
- `POST /checkout` — body `{ amount: number (USD), recurring: boolean }`. Creates a Stripe Checkout
  session (`payment` or `subscription` mode) and returns `{ url }` to redirect the donor to.
- `POST /webhook` — Stripe webhook receiver. Verifies the signature, then on
  `checkout.session.completed` records the donation and on `customer.subscription.deleted` marks the
  matching donations cancelled. Idempotent on event id.

Donor name, email, and phone are collected inside Stripe Checkout, so the landing form only asks for
an amount and frequency. "Manage donations" links directly to Stripe's hosted Customer Portal login
page (configured on the landing site via `PUBLIC_STRIPE_PORTAL_URL`); no server session is created.

## Local development

```bash
bun install
bun run db:migrate:local   # apply migrations to the local D1
bun run dev                # wrangler dev on http://localhost:8788
```

To exercise the webhook locally, forward Stripe events with the Stripe CLI:

```bash
stripe listen --forward-to http://localhost:8788/webhook
```

Put the signing secret it prints into `.dev.vars` as `STRIPE_WEBHOOK_SECRET`, along with a test
`STRIPE_SECRET_KEY`.

### Running via `./scripts/launch.sh --donate`

The repo's launch script can run this worker alongside the web/api/landing dev servers. Pass
`--donate` to enable it:

```bash
./scripts/launch.sh --donate            # localhost
./scripts/launch.sh --donate mydomain   # https://donate.mydomain.localhost via Caddy
```

Rather than editing `.dev.vars` each time, put shared secrets in `~/.config/virtue-dev.env`
(sourced by the launch script). It's read as shell, so use `KEY=value`:

```sh
STRIPE_SECRET_KEY=sk_test_...
PUBLIC_STRIPE_PORTAL_URL=https://billing.stripe.com/p/login/...   # landing "Manage donations" link
```

With `--donate`, the script also runs `stripe listen`, captures the webhook signing secret it
prints, and injects it as `STRIPE_WEBHOOK_SECRET` — so you don't need to set that yourself (it
falls back to the value in `~/.config/virtue-dev.env` if the Stripe CLI isn't available). These
launch-time values override whatever is in `.dev.vars`.

## Deploy / first-time setup

1. Create the D1 database and paste its id into `wrangler.json` (`d1_databases[0].database_id`):
   ```bash
   wrangler d1 create virtueinitiative-donate-db
   ```
2. Set secrets:
   ```bash
   wrangler secret put STRIPE_SECRET_KEY
   wrangler secret put STRIPE_WEBHOOK_SECRET
   ```
3. Apply migrations remotely and deploy:
   ```bash
   bun run db:migrate:remote
   bun run deploy
   ```
4. In the Stripe dashboard, add a webhook endpoint pointing at
   `https://donate-api.virtueinitiative.org/webhook` (events: `checkout.session.completed`,
   `customer.subscription.deleted`) and enable the hosted Customer Portal login page; put that
   login URL into the landing site's `PUBLIC_STRIPE_PORTAL_URL`.

## Checks

```bash
bun run typecheck
bun test
bun run format:check
```
