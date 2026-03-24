import { defineWorkersConfig } from '@cloudflare/vitest-pool-workers/config';
import { TEST_JWT_PRIVATE_KEY, TEST_JWT_PUBLIC_KEY } from './test/jwt-test-keys';

export default defineWorkersConfig({
  test: {
    setupFiles: ['./test/setup.ts'],
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
          },
        },
      },
    },
    testTimeout: 10000,
  },
});
