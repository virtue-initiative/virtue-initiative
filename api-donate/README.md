# api-donate

Standalone Cloudflare Worker that powers the donations flow on the landing site. It creates
Stripe Checkout sessions and records completed donations in its own D1 database. It is intentionally
separate from the main `api/` worker and has no shared auth. The one shared piece of data is the
newsletter list (`newsletter_signups`), which lives in this database but is written by the main
`api/` worker through its `DONATE_DB` binding (see "Newsletter" below).

## Endpoints

- `GET /` — health check.
- `POST /checkout` — body `{ amount: number (USD), recurring: boolean }`. Creates a Stripe Checkout
  session (`payment` or `subscription` mode) and returns `{ url }` to redirect the donor to.
- `GET /goals` — active fundraising goals from the `goals` table, each with `targetCents` and
  `raisedCents`. Both are entered by hand in the `goals` table. Shown as a progress bar on the donate page.
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
(machine-wide) and/or `.env` at the repo root (per-worktree override — see `.env.example`).
These are the repo's general-purpose "set once, available everywhere" mechanism — see the
root `AGENTS.md` for the full list of recognized keys across `api`, `api-donate`, the client
build, and Android, plus `scripts/setup.sh`, which copies any of these values it finds into a
freshly created `.dev.vars`. Both files are read as shell, so use `KEY=value`:

```sh
STRIPE_SECRET_KEY=sk_test_...
PUBLIC_STRIPE_PORTAL_URL=https://billing.stripe.com/p/login/...   # landing "Manage donations" link
```

With `--donate`, the script also runs `stripe listen`, captures the webhook signing secret it
prints, and injects it as `STRIPE_WEBHOOK_SECRET` — so you don't need to set that yourself (it
falls back to the value in `~/.config/virtue-dev.env` if the Stripe CLI isn't available). These
launch-time values override whatever is in `.dev.vars`.

## Deploy

Like `api/`, this worker has separate `staging` and `prod` environments defined in
`wrangler.json`, each with its own D1 database and route:

- `staging` → `staging-donate`, served at
  `staging.donate-api.virtueinitiative.org`
- `prod` → `donate`, served at `donate-api.virtueinitiative.org`

`.github/workflows/deploy.yml` deploys automatically on every push: `staging` on push to
`staging`, `prod` on push to `main`. No CI step runs D1 migrations — apply those manually
(see below) before code that depends on a schema change goes out.

### First-time setup per environment

1. Create the D1 database:
   ```bash
   wrangler d1 create donate-db           # prod
   wrangler d1 create staging-donate-db    # staging
   ```
   and paste the resulting id into the matching `env.prod`/`env.staging` block in
   `wrangler.json`.
2. Point a proxied (orange-clouded) DNS record in the `virtueinitiative.org` zone at each
   hostname above — Worker routes only intercept traffic for hostnames that already have a
   DNS record in the zone; they don't create one.
3. Set secrets per environment:
   ```bash
   wrangler secret put STRIPE_SECRET_KEY --env staging
   wrangler secret put STRIPE_WEBHOOK_SECRET --env staging
   wrangler secret put STRIPE_SECRET_KEY --env prod
   wrangler secret put STRIPE_WEBHOOK_SECRET --env prod
   ```
4. Apply migrations remotely:
   ```bash
   bun run db:migrate:staging
   bun run db:migrate:prod
   ```
5. In the Stripe dashboard, add a webhook endpoint per environment pointing at
   `https://<host>/webhook` (events: `checkout.session.completed`,
   `customer.subscription.deleted`) and enable the hosted Customer Portal login page; put that
   login URL into the landing site's `PUBLIC_STRIPE_PORTAL_URL` for that environment.

## Newsletter

The `newsletter_signups` table holds one row per email, with the country and region (a US state, for
example) Cloudflare reported when it was added, or the next time that account logged in. The main
`api/` worker adds rows from the landing form (`POST /newsletter`, API-058) and from the opt-in box at
account signup (API-010). Nothing here serves it.

`scripts/newsletter.ts` manages the list and sends issues. Run it from `api-donate/`:

```bash
# Write a preview (issue.html) and count the audience. Sends nothing.
bun run newsletter send issue.md --env prod --country US --region CA
# Send a test to one address.
bun run newsletter send issue.md --env prod --to you@example.com
# Send to everyone who matches, after a prompt. Reruns skip addresses in issue.sent.txt.
bun run newsletter send issue.md --env prod --send
# Remove someone who asked to unsubscribe.
bun run newsletter unsubscribe reader@example.com --env prod
# One-time copy of Buttondown's active subscribers.
BUTTONDOWN_API_KEY=... bun run newsletter import-buttondown --env prod
```

The issue's first `# Heading` is the subject. `--env local` uses the local D1 instead.

Sending needs `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` for the `virtue-newsletter` IAM user,
which can only send from `news.virtueinitiative.org` through the `newsletter` SES configuration set.
Mail goes from `newsletter@news.virtueinitiative.org` with a `Reply-To` of
`help@virtueinitiative.org`. Each email says to unsubscribe by replying, and carries a matching
`List-Unsubscribe: mailto:` header. Those replies arrive at help@, so run `unsubscribe` for each one.

## Checks

```bash
bun run typecheck
bun test
bun run format:check
```
