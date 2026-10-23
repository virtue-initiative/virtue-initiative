import { defineWorkersConfig } from '@cloudflare/vitest-pool-workers/config';

export default defineWorkersConfig({
  test: {
    setupFiles: ['./test/setup.ts'],
    poolOptions: {
      workers: {
        wrangler: { configPath: './wrangler.json', environment: 'staging' },
        miniflare: {
          bindings: {
            STRIPE_SECRET_KEY: 'sk_test_dummy',
            STRIPE_WEBHOOK_SECRET: 'whsec_test_secret',
            LANDING_URL: 'http://localhost:4321',
            BUTTONDOWN_API_KEY: 'bd-key',
          },
        },
      },
    },
    testTimeout: 10000,
  },
});
