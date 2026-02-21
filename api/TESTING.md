# Testing

## Overview

Tests run in a plain Node.js environment via [Vitest](https://vitest.dev/). There is **no Cloudflare Workers runtime** in the test environment — D1, R2, and other bindings are not available. This is a tradeoff: it means tests run fast and without any Cloudflare setup, but route-level integration tests require mocking the Hono app manually.

## Running Tests

```bash
npm test          # run all tests once
npm run test:watch  # re-run on file changes
```

## Structure

```
test/
  auth.test.ts      # unit tests — password hashing and JWT
  devices.test.ts   # stub (TODO)
  images.test.ts    # stub (TODO)
  logs.test.ts      # stub (TODO)
  partners.test.ts  # stub (TODO)
  settings.test.ts  # stub (TODO)
```

## What's Actually Tested

### `auth.test.ts` (real tests)

These test the crypto utilities directly, with no HTTP layer or database:

- **Password hashing** — `hashPassword` + `verifyPassword` using Argon2id
  - Verifies correct password is accepted
  - Verifies wrong password is rejected
  - Verifies two hashes of the same password are different (salted)
- **JWT tokens** — `generateAccessToken` + `verifyJWT` using `jose`
  - Verifies token round-trips with correct payload (`sub`, `type`)
  - Verifies token is rejected if the secret is wrong
  - Verifies token is rejected after expiry (generates a 1-second token, waits 1.5s)

> **Note:** Argon2id hashing uses production parameters (`t:3, m:65536`), which takes ~7–11 seconds per hash. The test timeout is set to **30 seconds** in `vitest.config.ts` to accommodate this.

### All other test files (stubs)

The remaining files contain `expect(true).toBe(true)` placeholders. They exist to document what *should* be covered but aren't yet.

## Why No Integration Tests?

The ideal integration test setup for Cloudflare Workers is [`@cloudflare/vitest-pool-workers`](https://developers.cloudflare.com/workers/testing/vitest-integration/), which runs tests inside a real Workers runtime with D1/R2/KV mocks. However, that package currently requires Vitest **2.x–3.2.x**, and this project uses Vitest **4.x**, making them incompatible.

Options to enable full integration testing:

1. **Downgrade Vitest to 3.x** — allows `@cloudflare/vitest-pool-workers`, enables real D1/R2 bindings in tests
2. **Use Miniflare directly** — spin up a local Workers environment in test setup without the pool workers package
3. **HTTP integration tests** — run `wrangler dev` locally and test against it with `fetch`

## Adding New Tests

To add a real test to one of the stub files, replace the `expect(true).toBe(true)` placeholder. For route tests without a Workers runtime, you can import the Hono app and call it directly:

```ts
import app from '../src/index';

it('should return 401 without auth', async () => {
  const res = await app.request('/device', { method: 'GET' });
  expect(res.status).toBe(401);
});
```

This works for testing routing and middleware logic, but any handler that reads from `env.DB` (D1) or `env.R2` will throw because those bindings are `undefined` in the Node environment.
