# Cleanup: old Cloudflare account (`cloudflare@anb.codes`)

Follow-up to the migration to the dedicated `help@virtueinitiative.org` account
(account ID `5bf62eb0ef0229b690dfeb2513b5bd54`). This document covers deleting
the leftover `virtueinitiative-*` resources from the old personal account
(account ID `79d73bbcd0d98228abaa048eac36ca3c`).

## When to run this

Not before the new account has soaked in production for **24-48 hours**,
covering:

- At least one successful `api` hourly cron invocation (`5 * * * *`) — check
  the Workers dashboard invocation logs for the `api` Worker.
- At least one full push-triggered GitHub Actions deploy that succeeded
  end-to-end using the new `CLOUDFLARE_ACCOUNT_ID`/`CLOUDFLARE_API_TOKEN`.

If either of those hasn't happened yet, wait.

## Pre-flight checklist (do this before deleting anything)

- [ ] `app.virtueinitiative.org`, `virtueinitiative.org`, `donate-api.virtueinitiative.org`,
      and `api.virtueinitiative.org/.well-known/jwks.json` all return 200 from the
      **new** account.
- [ ] `wrangler d1 migrations list DB --env <env> --remote` shows no gaps for
      both `api` and `api-donate`, both envs, on the new account.
- [ ] No errors in the new account's Workers Logs for `api`/`api-donate`/`web`/`landing`
      over the soak window.
- [ ] A real donation (or Stripe test-mode dry run) has completed successfully
      against the new account since cutover.

Only proceed once every box is checked.

## What to delete (old account only)

Use `CLOUDFLARE_ACCOUNT_ID=79d73bbcd0d98228abaa048eac36ca3c` for every command
below. **Do not** touch anything else in this account — it also hosts
unrelated personal projects (`auditv2`, `christmashunt` D1 databases,
`dev-drive`, `obsidian` R2 buckets, etc.). Only delete the resources named here.

### Workers

```bash
export CLOUDFLARE_ACCOUNT_ID=79d73bbcd0d98228abaa048eac36ca3c

wrangler delete --name virtueinitiative-web
wrangler delete --name virtueinitiative-staging-web
wrangler delete --name virtueinitiative-api
wrangler delete --name virtueinitiative-staging-api
wrangler delete --name virtueinitiative-landing
wrangler delete --name virtueinitiative-staging-landing
wrangler delete --name virtueinitiative-donate-api
wrangler delete --name virtueinitiative-donate-staging-api
```

Deleting a Worker with a `custom_domain` route also releases its DNS record,
but the zone itself already moved to the new account during the migration, so
this is just cleaning up the Worker script — it has no live traffic on this
account anymore.

### D1 databases

```bash
export CLOUDFLARE_ACCOUNT_ID=79d73bbcd0d98228abaa048eac36ca3c

wrangler d1 delete virtueinitiative-db                    # ff636ee0-a8f9-44a1-8a16-f0a162cf1c73
wrangler d1 delete virtueinitiative-staging-db             # 9ec06359-1165-48bc-a73a-a870d9082980
wrangler d1 delete virtueinitiative-donate-db              # 9c635213-47e5-4a98-840b-e175403dc4b7
wrangler d1 delete virtueinitiative-donate-staging-db      # e7ab8f0c-34aa-4979-ae75-75605fa70014
```

### R2 buckets

The old buckets may still have custom domain bindings
(`r2.virtueinitiative.org` / `staging.r2.virtueinitiative.org`) left over from
before the zone moved. Remove those first, then delete the buckets — R2 won't
let you delete a non-empty bucket, so empty it first if needed.

```bash
export CLOUDFLARE_ACCOUNT_ID=79d73bbcd0d98228abaa048eac36ca3c

wrangler r2 bucket domain remove virtueinitiative-images --domain r2.virtueinitiative.org
wrangler r2 bucket domain remove virtueinitiative-staging-images --domain staging.r2.virtueinitiative.org

# empty + delete (mirrors api/scripts/clean-remote.mjs, but for the OLD account/bucket names)
wrangler r2 bucket delete virtueinitiative-images
wrangler r2 bucket delete virtueinitiative-staging-images
```

If either bucket delete fails because it's non-empty, list and delete objects
first (see `api/scripts/clean-remote.mjs` for the pattern — you'll need a
one-off variant pointed at the old bucket name/account, since that script now
targets the new account's `app-bucket`/`staging-app-bucket`).

## After deleting

- Double check nothing else in the old account references these names
  (`wrangler whoami`, `wrangler d1 list`, `wrangler r2 bucket list` against
  the old account ID should come back clean of `virtueinitiative-*` entries).
- Delete this file (`CLEANUP-OLD-ACCOUNT.md`) once cleanup is confirmed done —
  it has no further purpose after this point.
