# api-donate

Standalone Cloudflare Worker that powers the donations flow on the landing site. It creates
Stripe Checkout sessions and records completed donations in its own D1 database. It is intentionally
separate from the main `api/` worker and has no shared auth or data.

## Endpoints

- `GET /` — health check.
- `POST /checkout` — body `{ amount: number (USD), recurring: boolean }`. Creates a Stripe Checkout
  session (`payment` or `subscription` mode) and returns `{ url }` to redirect the donor to.
- `GET /goals` — active fundraising goals from the `goals` table, each with `targetCents` and
  `raisedCents`. Both are entered by hand in the `goals` table. Shown as a progress bar on the donate page.
- `POST /newsletter` — form body `email` and `cf-turnstile-response`. Landing page newsletter signup.
  Rejects the post (redirect to `/newsletter/error` on the landing site) unless it passes a per-IP
  rate limit (5 a minute), a Cloudflare Turnstile check, and a daily cap on confirmation emails
  (`NEWSLETTER_DAILY_CONFIRMATION_CAP`, default 200). Otherwise it stores the visitor's country and
  region (Cloudflare geoip) in `newsletter_subscribers`, emails a confirmation link through SES, and
  redirects to `/newsletter/check-email`. An invalid address redirects back to the form. At most one
  confirmation email goes out per address per hour (per day once the address has unsubscribed, though
  the first request after an unsubscribe always goes through), never to a suppressed address, and the
  visitor sees the same redirect either way.
- `GET|POST /newsletter/confirm?token=` — the link in the confirmation email. `GET` shows a button
  (mail scanners fetch links), `POST` confirms and redirects to `/newsletter/confirmed`. A link works
  only for the latest confirmation email sent to that address, only if the address has not unsubscribed
  since, and never for a suppressed address.
- `POST /newsletter/sns` — SES bounce and complaint events from SNS. Verifies the SNS signature, then
  suppresses addresses that hard-bounced or complained. Transient bounces and messages that are not SES
  events are ignored.
- `GET|POST /newsletter/unsubscribe?token=` — the link in every newsletter. `GET` shows a button,
  `POST` unsubscribes and also serves RFC 8058 one-click clients.
- `POST /webhook` — Stripe webhook receiver. Verifies the signature, then on
  `checkout.session.completed` records the donation and on `customer.subscription.deleted` marks the
  matching donations cancelled. Idempotent on event id.

Donor name, email, and phone are collected inside Stripe Checkout, so the landing form only asks for
an amount and frequency. "Manage donations" links directly to Stripe's hosted Customer Portal login
page (configured on the landing site via `PUBLIC_STRIPE_PORTAL_URL`); no server session is created.

## Newsletter

Subscribers live in `newsletter_subscribers`. An address receives mail only when `confirmed_at` is set
and neither `unsubscribed_at` nor `suppressed_at` is. Landing signups confirm by email link, and
accounts that tick the box when signing up (API-010) are confirmed immediately, which also resubscribes
an address that had unsubscribed.

`unsubscribed_at` is the subscriber's own choice and can be undone by confirming or opting in again.
`suppressed_at` is set by a hard bounce or complaint and nothing undoes it: the signup form never
emails the address, a confirm link is refused, and an account opt-in leaves it alone. Confirm and
unsubscribe links are stateless HMAC tokens signed with `NEWSLETTER_TOKEN_SECRET`. A confirm link also
carries the time of the email it was sent in, which has to match the address's latest confirmation.

The list is separate from accounts on purpose. Deleting an account or changing its email address does
not touch the list: the old address was subscribed by whoever controls it, every email carries an
unsubscribe link, and a removal request is a row update (`unsubscribed_at = unixepoch()`). Tying the
two together would also unsubscribe people who still read the mail at the old address.

### Sending a newsletter

Write a markdown file with the subject in front matter:

```markdown
---
subject: October update
---

# Hello

Body in **markdown**.
```

Then run it from `api-donate/`. The default is a dry run that prints the audience size and writes
`<file>.preview.html`:

```bash
bun run newsletter october.md --env prod --country US --region CA
bun run newsletter october.md --env prod --region CA --test-to you@example.com
bun run newsletter october.md --env prod --region CA --send
```

Filters are `--country`, `--region` (ISO 3166-2 code), `--source` (`landing` or `signup`),
`--since` and `--before`. See `bun run newsletter --help`. The script needs wrangler logged in to
read the D1 database, plus `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `NEWSLETTER_FROM_EMAIL` and
the target worker's `NEWSLETTER_TOKEN_SECRET` in the environment, and optionally
`NEWSLETTER_SES_CONFIG_SET` (default `newsletter`). Every footer carries the organization's postal
address, which `NEWSLETTER_POSTAL_ADDRESS` overrides. Replies go to `NEWSLETTER_REPLY_TO` (default
`help@virtueinitiative.org`), since the From address has no mailbox.

Safeguards in the script:

- Before sending anything, it requests the first generated unsubscribe link from the target worker and
  aborts unless that returns 200. This catches a `NEWSLETTER_TOKEN_SECRET` that belongs to another
  environment. `NEWSLETTER_WORKER_URL` points it at a dev worker on a different port.
- A `--send` appends every address it reaches to `<file>.sent.jsonl` next to the markdown file. If a
  run crashes or some sends fail, run the same command again and it mails only the addresses not yet
  logged for that target (staging and prod are tracked separately). Delete the log to send the issue to everyone again. A `--test-to` send is not logged.

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
   For the newsletter, also follow [Newsletter email setup](#newsletter-email-setup).
4. Apply migrations remotely:
   ```bash
   bun run db:migrate:staging
   bun run db:migrate:prod
   ```
5. In the Stripe dashboard, add a webhook endpoint per environment pointing at
   `https://<host>/webhook` (events: `checkout.session.completed`,
   `customer.subscription.deleted`) and enable the hosted Customer Portal login page; put that
   login URL into the landing site's `PUBLIC_STRIPE_PORTAL_URL` for that environment.

## Newsletter email setup

Do this once per environment (`staging`, `prod`), in the AWS account that environment sends from. The
worker and the send script both mail through SES in `us-east-1`. Newsletter mail goes from its own
subdomain, `news.virtueinitiative.org`, so complaints about newsletters can't damage the reputation
of the domain that sends password resets and alerts. The `virtueinitiative.org` DNS zone is on
Cloudflare, so every record below is added there with the proxy off (grey cloud).

### 1. Verify the sending domain in SES

1. SES console, region `us-east-1`, then Identities, Create identity, Domain. Enter
   `news.virtueinitiative.org`, leave Easy DKIM on (RSA 2048), and set a custom MAIL FROM domain of
   `bounce.news.virtueinitiative.org` with "Reject message" on MX failure.
2. Add the DNS records SES lists: three DKIM `CNAME`s, one `MX` for `bounce.news` (priority 10,
   `feedback-smtp.us-east-1.amazonses.com`), and a `TXT` on `bounce.news` with
   `v=spf1 include:amazonses.com ~all`.
3. Add a DMARC record, a `TXT` on `_dmarc.news.virtueinitiative.org`:
   `v=DMARC1; p=none; rua=mailto:help@virtueinitiative.org`. Tighten `p=` to `quarantine` once
   reports look clean.
4. Wait until the identity shows Verified and DKIM shows Successful. If the account is still in the
   SES sandbox, request production access (the existing account that sends account email already has
   it).
5. Update how AWS sees this account. It was approved for transactional mail only, and newsletters are
   marketing mail, so a bad newsletter could get the whole account reviewed or paused, which would
   stop password resets and alerts too. Open a Support case (Create case, Service limit increase, SES Sending Limits, region US East N. Virginia) saying that you are adding marketing or newsletter mail, with the website URL, and describing the use as a double opt-in newsletter sent from a separate subdomain, with confirmation emails,
   one-click unsubscribe, and automatic suppression of hard bounces and complaints. Give the expected
   volume. Check the daily sending quota and send rate on the SES Account dashboard and ask for an increase in the same case if
   the quota is below your list size. The send script sends 10 messages per second.

### 2. Create the bounce and complaint pipeline

1. SNS, Topics, Create topic, type Standard, name `virtue-newsletter-events`. In its access policy,
   allow SES to publish:

   ```json
   {
     "Sid": "AllowSesPublish",
     "Effect": "Allow",
     "Principal": { "Service": "ses.amazonaws.com" },
     "Action": "SNS:Publish",
     "Resource": "<topic ARN>",
     "Condition": { "StringEquals": { "AWS:SourceAccount": "<account id>" } }
   }
   ```

2. SES, Configuration sets, Create set named `newsletter` (the default the worker and script expect).
3. In that set, Event destinations, Add destination. Select only the Bounce and Complaint event
   types, destination type Amazon SNS, and choose `virtue-newsletter-events`.
   Then open the set's Reputation options and turn on reputation metrics, so newsletter bounce and
   complaint rates are tracked on their own and sending for this set alone can be paused (from a
   CloudWatch alarm) without touching account email.
4. The topic ARN has to match `SNS_TOPIC_ARNS` in `wrangler.json`
   (`arn:aws:sns:us-east-1:<account>:virtue-newsletter-events`). Both environments use the one SES account (`599243104123`), so both point at the same topic.

Do the subscription in step 6, after the worker is deployed.

### 3. Create an IAM user that can only send newsletters

IAM, Users, Create user `virtue-newsletter`, with no console access and this inline policy (use your
account id; this allows only that identity and configuration set):

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": "ses:SendEmail",
      "Resource": [
        "arn:aws:ses:us-east-1:<account id>:identity/news.virtueinitiative.org",
        "arn:aws:ses:us-east-1:<account id>:configuration-set/newsletter"
      ]
    }
  ]
}
```

Create an access key for it. The worker uses this key for confirmation emails and you use it when
running the send script.

### 4. Protect the signup form

The form is public and every confirmation email counts against the one SES account that also sends
password resets and alerts, so it has three guards. A bot mailing nonexistent addresses could push
that account past AWS's bounce limits (review starts around 5%), so don't skip this step.

1. **Turnstile.** Cloudflare dashboard, Turnstile, Add widget. Name it "Newsletter signup", add the
   hostnames `virtueinitiative.org` and `staging.virtueinitiative.org` (subdomains are covered), and
   pick the Managed mode. Copy the site key and the secret key.
2. **Set the secret on the worker.** The worker refuses every signup without it:

   ```bash
   wrangler secret put TURNSTILE_SECRET_KEY --env staging   # and --env prod
   ```

3. **The site key is committed.** `PUBLIC_TURNSTILE_SITE_KEY` is in `landing/.env.production` and
   `landing/.env.staging` (site keys are public), and a production landing build fails if it is empty.
   If you create a new widget, update both files. The widget sets `data-action="newsletter"`, and the
   worker only accepts tokens for that action issued on a host in `TURNSTILE_HOSTNAMES` (in
   `wrangler.json`; unset in local dev, where Cloudflare's test keys return fixed values).
4. **Rate limit and daily cap need no setup.** The per-IP limit is the `NEWSLETTER_RATE_LIMITER`
   binding in `wrangler.json` (5 posts a minute per IP; the namespace ids `2001` and `2002` must not
   be used by another worker in the account). The daily cap is `NEWSLETTER_DAILY_CONFIRMATION_CAP`,
   200 by default, and counts the confirmation emails sent in the last 24 hours across all addresses.
   Set it in the `vars` of `wrangler.json` if a campaign needs more.

### 5. Apply migrations, set secrets, deploy

Order matters. The main API writes the new columns when someone signs up with the newsletter box
ticked, and an opt-in that arrives before migration 0004 is applied is lost (it is only logged), so
apply the migrations to donate-db before either worker is deployed. Nothing enforces this order.

```bash
cd api-donate
bun run db:migrate:staging   # applies 0003 (if not yet) and 0004
bun run db:migrate:prod

# per environment, repeat with --env prod
openssl rand -hex 32 | wrangler secret put NEWSLETTER_TOKEN_SECRET --env staging
wrangler secret put AWS_ACCESS_KEY_ID --env staging
wrangler secret put AWS_SECRET_ACCESS_KEY --env staging
```

Keep the `NEWSLETTER_TOKEN_SECRET` value somewhere safe (a password manager), because the send script
needs the same value to sign unsubscribe links. Rotating it invalidates every unsubscribe link already
sent, so don't. Then merge, and the deploy workflow ships `api-donate` and `api`. Note that pushes to
`staging` also deploy `api-donate` to prod, so migrate prod before merging.

### 6. Subscribe the worker to the topic

After the worker is deployed, SNS, Topics, `virtue-newsletter-events`, Create subscription: protocol
HTTPS, endpoint `https://staging.donate-api.virtueinitiative.org/newsletter/sns` (or
`https://donate-api.virtueinitiative.org/newsletter/sns` for prod). The worker verifies the SNS
signature and confirms the subscription itself, so the status flips to Confirmed within seconds. If
it stays Pending, check the worker logs.

### 7. Verify end to end

1. Submit the form on the landing page with your own address. Posting the form without a Turnstile
   token (for example `curl -X POST -d email=you@example.com <worker>/newsletter`) should redirect to
   `/newsletter/error` and send nothing. You should get a confirmation email
   from `news.virtueinitiative.org`, and clicking it should land on `/newsletter/confirmed`.
2. Write a one-line markdown file with a `subject:` and send it to yourself:
   `bun run newsletter test.md --env staging --test-to you@example.com`. Check that the unsubscribe link
   in the footer works, and that Gmail's "Show original" shows `dkim=pass`, `spf=pass` and `dmarc=pass`.
3. Exercise bounce handling with the SES mailbox simulator, which doesn't count against your
   reputation. Insert confirmed rows for the simulator addresses, send to them, and confirm they
   become suppressed:

   ```bash
   bunx wrangler d1 execute DB --env staging --remote --command \
     "INSERT INTO newsletter_subscribers (email, source, confirmed_at) VALUES ('bounce@simulator.amazonses.com','landing',1),('complaint@simulator.amazonses.com','landing',1)"
   bun run newsletter test.md --env staging --test-to bounce@simulator.amazonses.com
   bun run newsletter test.md --env staging --test-to complaint@simulator.amazonses.com
   bunx wrangler d1 execute DB --env staging --remote --command \
     "SELECT email, suppressed_at FROM newsletter_subscribers WHERE email LIKE '%simulator.amazonses.com'"
   ```

   Both rows should have a `suppressed_at` within a minute. Delete them afterwards.

### Importing the existing Buttondown list

Addresses that subscribed through Buttondown are not in D1. Only import people you may email under
the terms they subscribed with, and consider asking them to re-confirm instead. To import them as
already confirmed, export the subscribed addresses from Buttondown and insert them with
`confirmed_at = unixepoch()`, `source = 'landing'`.

### Watch the numbers

In SES, Reputation metrics shows the bounce and complaint rates. AWS puts an account under review at
roughly 5% bounces or 0.1% complaints and pauses sending at roughly 10% or 0.5%, so keep bounces
under 2% and complaints well under 0.1%. Hard bounces and complaints are already suppressed through `/newsletter/sns` and never emailed again, so
a rate above that means the list itself needs attention.

## Checks

```bash
bun run typecheck
bun test
bun run format:check
```
