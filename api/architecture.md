# API architecture

This page explains how the API is put together: the path a request takes, what the API stores and
where, the jobs it runs on a schedule, and the email it sends. In one sentence, the API is one
Cloudflare Worker that keeps accounts and encrypted batches in D1 and R2, never reads what is
inside a batch, and does nearly all its work either inside a request or in an hourly job. Every
endpoint is specified in [`SPEC.md`](SPEC.md), [`README.md`](README.md) covers running and
deploying it, and [`AGENTS.md`](AGENTS.md) lists the conventions that are easy to break.

## A request, start to finish

Everything enters through the Hono app in `src/index.ts`. Before routing, it strips two optional
prefixes from the path: `API_BASE_PATH` (so `virtueinitiative.org/api/...` and
`api.virtueinitiative.org/...` reach the same routes) and the version prefix clients send
(API-005, `src/lib/api-version.ts`). Then the request goes to one of the route files in
`src/routes/`, mounted by path: `/d` is the device API, `/data` serves batches, `/device`,
`/locked-password`, `/admin` and `/dev` are what they sound like, and the account, partner,
newsletter, bug report and email webhook routes sit at the root.

Most routes then pass through the same few pieces of middleware in `src/middleware/`. `auth.ts`
accepts a web session (the `refresh_token` cookie, or the same token as a bearer header) or a
device token, and the two are kept apart, so a device token cannot reach a web route or the
reverse (API-003). `rate-limit.ts` limits by IP on the account, bug report and newsletter routes, and by device
on `/d/*` (API-034). `validation.ts` checks the body or query against a Zod schema and returns the
standard 400 when it does not match. Every D1 query lives in `src/lib/db.ts`, which keeps the SQL
in one place and makes it much easier to see what an index is for.

## What it stores

The API keeps its state in three places. One D1 database holds every row the API reads or
writes, a second holds the newsletter list it shares with `api-donate/`, and R2 holds the
encrypted batch files.

| Where          | What                                                                                                                                                                                                                                                                                            |
| -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D1 `DB`        | `users` (with the public key and encrypted private key), `user_sessions`, `devices`, `device_sessions`, `partners`, `batches` (one row per batch, with its wrapped keys and end hash), `email_tokens`, `email_bounces`, `locked_passwords`, `feature_flags`, `admins` and `analytics_snapshots` |
| D1 `DONATE_DB` | The newsletter list, shared with `api-donate/`                                                                                                                                                                                                                                                  |
| R2 `BUCKET`    | The encrypted batch files, and nothing else. A batch's key and URL come from its version, owner and id (`src/lib/batch-url.ts`)                                                                                                                                                                 |

The hash server is a fourth place, but the API does not own it. It keeps each device's running
hash, and the API only asks for it and resets it (`src/lib/hash-server.ts`).

> Note: the API **cannot** read a batch, but it can see a fair amount about one: who uploaded it
> and when, which device it came from, how many events, screenshots and high and medium risk
> events it holds (`event_counts`), and the title and details of any alert sent with it. That is
> what the digests and alerts are built from. Decision
> [0001](../docs/decisions/0001-servers-never-read-content.md) explains where that line is drawn
> and why.

The schema is the sum of the migrations in `src/schema/migrations/`. Neither CI nor the deploy
scripts apply them, so a change that adds one needs `bun run db:migrate:staging` and
`bun run db:migrate:prod` run by hand before the code that needs it deploys (see
[`docs/releasing.md`](../docs/releasing.md)). The tests do not run the migrations: `test/setup.ts`
keeps its own copy of the schema, so a migration needs the same change made there too.

## A batch arriving

`POST /d/batch` (API-038) is the busiest route and touches every store, so it is worth following
once. The device sends a multipart form with the encrypted file and a JSON `metadata` part. The
handler first asks the hash server for the device's current hash, using a 60-second `server` JWT,
and refuses the upload if that does not come back as a proper hash. It then writes the file to R2
and the row to D1. If the device was deleted while the upload was in flight, it deletes the file
again and answers 404. Next it resets the device's hash on the hash server, so the next batch's
chain starts fresh, and sends an alert email to the owner's partners for each entry in
`notifications`. Last, it answers with the batch and a fresh copy of the device's settings, which
is how a device learns about a new partner or a new hash token without asking separately.

## Scheduled jobs

Two crons in `wrangler.json` drive everything that is not a request, and `scheduled` in
`src/index.ts` runs only the jobs for the one that fired.

| Cron                 | Jobs                                                                                                                                                                                            |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `5 * * * *` (hourly) | Daily and weekly digest emails (`runNotificationSchedule` in `src/lib/scheduler.ts`, API-057), and pruning expired email tokens, sessions and deleted locked passwords (`src/lib/retention.ts`) |
| `20 0 * * *` (daily) | Deleting batch rows older than 30 days (`pruneExpiredBatches`), then storing the day's analytics snapshot (`src/lib/analytics.ts`, API-051)                                                     |

The batch files in R2 are not deleted by any of this. The bucket has a lifecycle rule that expires
objects after 30 days, set once per bucket with `wrangler r2 bucket lifecycle add`, and nothing in
the repo creates it, so a new bucket needs it added by hand. `api/AGENTS.md` shows how to fire a
cron against a local dev server.

## Email

All email goes through `sendEmail` in `src/lib/email.ts`, which sends through Amazon SES and
builds each message from `src/lib/email/templates.ts`. It sends four kinds: account email
(signup links, password resets, email verification), partner invites, alerts that go out at once
(an alert from a batch, a device signing out or being deleted, a locked password being revealed),
and the digests. A few rules apply to all of them:

- With `EMAIL_DELIVERY_MODE=log` (the local default) nothing is sent, and each email is printed to
  the dev server log instead. `GET /dev/emails` (API-066) renders every template with sample data.
- SES reports bounces to an SNS topic that calls `POST /email/sns` (API-041). An address with a
  permanent bounce is never emailed again (API-056), and the signup and invite flows tell the
  person why.
- A send that fails with a 5xx is retried a few times with backoff (API-067).
- The hourly digest job paces itself to stay under SES's sending rate, leaving room for the alerts
  and account email that go out while it runs.

## Where to look

| What                                 | File                                                 |
| ------------------------------------ | ---------------------------------------------------- |
| App setup, routing and the crons     | `src/index.ts`                                       |
| Routes                               | `src/routes/`                                        |
| Auth, rate limits, validation, admin | `src/middleware/`                                    |
| Every D1 query                       | `src/lib/db.ts`                                      |
| Password hashing and fake salts      | `src/lib/password.ts`                                |
| Tokens and JWTs                      | `src/lib/tokens.ts`, `src/lib/jwt.ts`                |
| The hash server client               | `src/lib/hash-server.ts`                             |
| Digests and their timing             | `src/lib/scheduler.ts`, `src/lib/digest-schedule.ts` |
| Email sending and templates          | `src/lib/email.ts`, `src/lib/email/templates.ts`     |
| Feature flags                        | `src/lib/feature-flags.ts`                           |
| Bindings and environment             | `src/types/bindings.ts`, `wrangler.json`             |

The tests send real HTTP requests to the Worker with real local D1 and R2, and
[`TESTING.md`](TESTING.md) explains how. That is also the quickest way to see any route on this
page actually run.
