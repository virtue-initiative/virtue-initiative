import { defineWorkersConfig } from '@cloudflare/vitest-pool-workers/config';

export default defineWorkersConfig({
  test: {
    setupFiles: ['./test/setup.ts'],
    poolOptions: {
      workers: {
        wrangler: { configPath: './wrangler.toml' },
        miniflare: {
          bindings: {
            JWT_SECRET: 'test-secret-key-for-testing-only',
            AWS_ACCESS_KEY_ID: 'test-aws-key',
            AWS_SECRET_ACCESS_KEY: 'test-aws-secret',
            EMAIL_DELIVERY_MODE: 'log',
          },
        },
      },
    },
    testTimeout: 10000,
  },
});
