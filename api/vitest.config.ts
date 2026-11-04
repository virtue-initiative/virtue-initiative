import { defineWorkersConfig } from '@cloudflare/vitest-pool-workers/config';
import { writeWranglerConfig } from '../scripts/cf-config';
import { TEST_JWT_PRIVATE_KEY, TEST_JWT_PUBLIC_KEY } from './test/jwt-test-keys';

// The pool reads bindings from a Wrangler config file, so it gets staging's from
// cloudflare.config.ts through one generated in .cloudflare/.
const configPath = await writeWranglerConfig('api', 'staging');

export default defineWorkersConfig({
  test: {
    setupFiles: ['./test/setup.ts'],
    poolOptions: {
      workers: {
        wrangler: { configPath },
        miniflare: {
          bindings: {
            JWT_PRIVATE_KEY: TEST_JWT_PRIVATE_KEY,
            JWT_PUBLIC_KEY: TEST_JWT_PUBLIC_KEY,
            AWS_ACCESS_KEY_ID: 'test-aws-key',
            AWS_SECRET_ACCESS_KEY: 'test-aws-secret',
            EMAIL_DELIVERY_MODE: 'log',
            // As in local dev, so batch URLs point at GET /r2/* and it is served (API-040).
            R2_URL: 'http://localhost/r2',
            DEV_R2_ROUTE: 'true',
            // Checked by the siteverify stub in test/setup.ts.
            TURNSTILE_SECRET_KEY: 'test-turnstile-secret',
            LOGIN_SALT_SECRET: 'test-login-salt-secret',
            SNS_TOPIC_ARNS:
              'arn:aws:sns:us-east-1:111111111111:old-ses-events, arn:aws:sns:us-east-1:222222222222:ses-events',
            // Intercepted by fetchMock's hash-server test double — see test/hash-server-mock.ts.
            HASH_SERVER_URL: 'https://example.com',
          },
        },
      },
    },
    testTimeout: 10000,
  },
});
