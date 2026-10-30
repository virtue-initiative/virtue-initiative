# API testing

See the root `TESTING.md` for the conventions every test follows, including citing an
`API-xxx` section in each test's title.

## Running tests

```bash
bun run test        # run all tests once (vitest run)
bun run test:watch  # re-run on file changes
```

## Setup

Tests run inside the Workers runtime through `@cloudflare/vitest-pool-workers`
(`vitest.config.ts`), using the `staging` environment from `wrangler.json`. D1 and R2
are real local bindings. `test/setup.ts` creates the schema. Test-only
bindings (JWT keys, Turnstile secret, SNS topics) are set in `vitest.config.ts`.

- Send requests with `SELF.fetch` and assert on the response and on effects other
  endpoints can see. Read D1 or R2 directly only for effects with no endpoint, such as
  stored metadata or deleted objects.
- `test/helpers.ts` has the shared steps: `clearDB` (run in `beforeEach`),
  `signupAndGetCookie`, `createDeviceForUser`, `batchMetadataForm`, and email helpers
  (`EMAIL_DELIVERY_MODE=log` records deliveries instead of sending them).
- Outbound HTTP is intercepted with `fetchMock` from `cloudflare:test`.
  `test/hash-server-mock.ts` is a test double for the hash server, and `test/setup.ts`
  stubs Turnstile's siteverify.
- The `scheduled` handler is called directly with `createExecutionContext` and
  `waitOnExecutionContext` (see `analytics.test.ts`).
