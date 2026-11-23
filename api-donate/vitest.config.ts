import { defineWorkersConfig } from '@cloudflare/vitest-pool-workers/config';
import { writeWranglerConfig } from '../scripts/cf-config';

// The pool reads bindings from a Wrangler config file, so it gets staging's from
// cloudflare.config.ts through one generated in .cloudflare/.
const configPath = await writeWranglerConfig('api-donate', 'staging');

export default defineWorkersConfig({
  test: {
    setupFiles: ['./test/setup.ts'],
    poolOptions: {
      workers: {
        wrangler: { configPath },
        miniflare: {
          bindings: {
            STRIPE_SECRET_KEY: 'sk_test_dummy',
            STRIPE_WEBHOOK_SECRET: 'whsec_test_secret',
            LANDING_URL: 'http://localhost:4321',
          },
        },
      },
    },
    testTimeout: 10000,
  },
});
