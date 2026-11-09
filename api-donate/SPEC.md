# Virtue Initiative Donations API

## DONATE-001 Overview

This spec defines the standalone donations server, a Cloudflare Worker (TypeScript, Hono, D1) separate from the main API (`api/SPEC.md`). It creates Stripe Checkout sessions for the landing site's donate page, records completed donations from Stripe webhooks, and reports fundraising goal progress. It has no user accounts and no shared auth. Donor name, email and phone are collected inside Stripe Checkout, not by this server.

Its D1 database also hosts the newsletter table that the main API manages (DONATE-018).

### DONATE-002 Configuration

The server MUST be configured with these bindings:

- `DB`: the D1 database.
- `STRIPE_SECRET_KEY` and `STRIPE_WEBHOOK_SECRET`: secrets.
- `LANDING_URL`: base URL of the landing site, used for CORS and for Checkout redirect URLs.

`staging` and `prod` environments differ only in these values, their D1 databases and their hostnames. Behavior MUST be identical.

### DONATE-003 CORS

For every route except `POST /webhook`, the server MUST apply CORS (methods `GET`, `POST`, `OPTIONS`; header `Content-Type`). It MUST allow only the origin of `LANDING_URL`, plus any origin whose hostname is `localhost` or ends in `.localhost`. Other origins MUST NOT get `Access-Control-Allow-Origin`, which is what makes a browser refuse the response. A preflight from them MAY still list the allowed methods and header, since those aren't secret. `POST /webhook` MUST NOT be CORS-gated, since Stripe calls it server to server.

### DONATE-004 Errors

An unknown route MUST get **HTTP 404** `{ "error": "Not found" }`. An unhandled exception MUST get **HTTP 500** `{ "error": "Internal server error", "details": { "message": string } }`.

> OPEN: the 500 body echoes the underlying exception message, which for Stripe failures is Stripe's error text — should `details` be dropped in production?

### DONATE-005 `GET /`

MUST return **HTTP 200** `{ "name": "Virtue Initiative Donations API", "version": "1.0.0", "status": "ok" }` as a health check.

### DONATE-006 `POST /checkout`

Creates a Stripe Checkout session and returns the URL to redirect the donor to. No authentication.

```json
// request (JSON)
{ "amount": 25, "recurring": false }
// response, HTTP 200
{ "url": "https://checkout.stripe.com/..." }
```

#### DONATE-007 Validation

`amount` MUST be a number greater than 0 and at most 100000, in whole or fractional US dollars. `recurring` MUST be a boolean. A request that fails this, including a non-positive amount, MUST get **HTTP 400** and MUST NOT call Stripe. The 400 body is the validator's default error body. A body that is not valid JSON MUST also get **HTTP 400**, with an `error` message.

> OPEN: the schema accepts fractional amounts, which are rounded to the nearest cent (`amount * 100`), so `0.001` becomes 0 cents and is rejected by Stripe as a 500, and Stripe's minimum charge is not checked — should the server enforce a minimum and whole cents, and return 400?

#### DONATE-008 Session contents

The server MUST create one Checkout session with a single line item of quantity 1, in USD, with `unit_amount = round(amount * 100)`.

- If `recurring` is false, the session mode MUST be `payment`.
- If `recurring` is true, the mode MUST be `subscription` and the price MUST recur with interval `month`.

The session MUST use `submit_type: donate`, require the donor's name, collect a phone number, and collect a billing address when Stripe deems it needed. `success_url` MUST be `{LANDING_URL}/donate/success?session_id={CHECKOUT_SESSION_ID}` and `cancel_url` MUST be `{LANDING_URL}/donate`.

> OPEN: the recurring product name is `a monthly donation to The Virtue Initiative`, which starts lowercase unlike the one-time `Donation to The Virtue Initiative` — is the lowercase `a` intentional?

#### DONATE-009 Checkout failures

If Stripe returns a session without a `url`, the server MUST respond **HTTP 502** `{ "error": "Failed to create checkout session" }`. If the Stripe call itself fails, the error falls through to DONATE-004 (HTTP 500).

### DONATE-010 `GET /goals`

Returns the active fundraising goals for the donate page's progress bar. No authentication.

```json
{
  "goals": [
    {
      "id": "lawyer",
      "title": "Legal fees",
      "description": "..." | null,
      "targetCents": 100000,
      "raisedCents": 0
    }
  ]
}
```

Only goals with `active = 1` MUST be returned, ordered by `sort_order` then `created_at`. A goal that is not active MUST be omitted, so the list MAY be empty. `raisedCents` MUST be the manually entered `raised_cents` value and is not derived from recorded donations. The migrations seed one goal, `lawyer`, with a target of 100000 cents and 0 raised.

### DONATE-011 `POST /webhook`

Receives Stripe events. The body is the raw Stripe event JSON and the request MUST carry a `stripe-signature` header. A processed event, including one of an unhandled type, MUST get **HTTP 200** `{ "ok": true }`.

#### DONATE-012 Signature verification

If the `stripe-signature` header is missing, the server MUST respond **HTTP 400** `{ "error": "Missing stripe-signature header" }`. It MUST verify the signature of the raw body against `STRIPE_WEBHOOK_SECRET` using Stripe's default timestamp tolerance. If verification fails, it MUST respond **HTTP 400** `{ "error": "Invalid signature" }`, and MUST NOT record the event id or change any donation.

#### DONATE-013 Idempotency

After verification the server MUST record the event id (with its type) in `stripe_events` before handling it. If the id was already recorded, it MUST respond **HTTP 200** `{ "ok": true, "duplicate": true }` and MUST NOT handle the event again. Event types that are not handled (DONATE-016) are still recorded.

> OPEN: the event id is recorded before the handler runs, so if the handler throws (for example a D1 error) the 500 makes Stripe retry, but the retry is seen as a duplicate and the donation is lost — should the id be recorded only after handling succeeds?

#### DONATE-014 `checkout.session.completed`

The server MUST insert a donation keyed by the Checkout session id, with status `completed`, from the session:

- `stripe_customer_id` and `stripe_subscription_id` (null when absent).
- `email`, `name` and `phone` from `customer_details` (null when absent).
- `amount_cents` from `amount_total` (0 when absent), and `currency` (default `usd`).
- `recurring` is true if and only if the session mode is `subscription`.

If a donation with that session id already exists, it MUST be overwritten with these values rather than duplicated.

> OPEN: the donation is recorded as `completed` without checking `payment_status`, so a delayed-payment method that has not yet paid would count as completed — should unpaid sessions be ignored until `checkout.session.async_payment_succeeded`?

#### DONATE-015 `customer.subscription.deleted`

The server MUST set the status to `cancelled` on every donation whose `stripe_subscription_id` equals the deleted subscription's id. If none match, it MUST do nothing and still respond HTTP 200.

> OPEN: only the first payment of a subscription is ever recorded as a donation (renewals send no handled event), so `donations` is not a full ledger of recurring gifts — is that intended, given `raised_cents` is entered by hand?

#### DONATE-016 Other events

Any other event type MUST be acknowledged with HTTP 200 and otherwise ignored (the Stripe dashboard is configured to send only the two above).

### DONATE-017 Data model

- `donations`: one row per completed Checkout session (`id` is the session id), with `status` (`completed` or `cancelled`) and `created_at`.
- `stripe_events`: processed event ids.
- `goals`: `id`, `title`, `description`, `target_cents`, `raised_cents`, `active`, `sort_order`, `created_at`.

Migrations in `src/schema/migrations/` MUST be applied manually before deploying code that depends on them.

### DONATE-018 Newsletter table

`newsletter_signups` (migration `0003`) lives in this database, but no route in this server reads or writes it. The main API writes it through its `DONATE_DB` binding (`API-058`, `API-059`, `API-060`; signup opt-in `API-010`; bounces and complaints `API-041`), and `scripts/newsletter.ts` reads it to send issues. Rows are keyed by lowercased email and carry the confirm and unsubscribe `token`, `country`, `region`, and the `created_at`, `confirmed_at`, `confirmation_sent_at` and `unsubscribed_at` timestamps. Only rows that are confirmed and not unsubscribed get newsletters.
