import { defineWorkersConfig } from '@cloudflare/vitest-pool-workers/config';

export default defineWorkersConfig({
  test: {
    setupFiles: ['./test/setup.ts'],
    poolOptions: {
      workers: {
        wrangler: { configPath: './wrangler.toml' },
        miniflare: {
          // Provide secrets that would normally come from `wrangler secret put`
          bindings: {
            JWT_SECRET: 'test-secret-key-for-testing-only',
            R2_ACCOUNT_ID: 'test-account-id',
            R2_ACCESS_KEY_ID: 'test-access-key',
            R2_SECRET_ACCESS_KEY: 'test-secret-key',
          },
        },
      },
    },
    testTimeout: 10000,
  },
});
