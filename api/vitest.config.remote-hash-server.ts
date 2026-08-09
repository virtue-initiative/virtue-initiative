import { defineWorkersConfig } from '@cloudflare/vitest-pool-workers/config';
import { TEST_JWT_PRIVATE_KEY, TEST_JWT_PUBLIC_KEY } from './test/jwt-test-keys';

// Dedicated config for device-cert-remote.test.ts: HASH_SERVER_URL here does
// NOT end in `/api`, so isLocalHashServer(env) is false and buildDeviceState
// takes the remote (device-cert-minting) branch. Kept separate from
// vitest.config.ts because HASH_SERVER_URL is baked into the worker's
// bindings at miniflare-instance startup — mutating the `env` object
// imported from cloudflare:test at runtime does not affect it.
export default defineWorkersConfig({
  test: {
    setupFiles: ['./test/setup.ts'],
    include: ['test/device-cert-remote.test.ts'],
    poolOptions: {
      workers: {
        wrangler: { configPath: './wrangler.json', environment: 'staging' },
        miniflare: {
          bindings: {
            JWT_PRIVATE_KEY: TEST_JWT_PRIVATE_KEY,
            JWT_PUBLIC_KEY: TEST_JWT_PUBLIC_KEY,
            AWS_ACCESS_KEY_ID: 'test-aws-key',
            AWS_SECRET_ACCESS_KEY: 'test-aws-secret',
            EMAIL_DELIVERY_MODE: 'log',
            HASH_SERVER_URL: 'https://hash.example.test',
          },
        },
      },
    },
    testTimeout: 10000,
  },
});
