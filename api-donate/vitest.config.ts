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
            NEWSLETTER_TOKEN_SECRET: 'newsletter-test-secret',
            TURNSTILE_SECRET_KEY: 'turnstile-test-secret',
            // Pinned so tests never pick up real credentials from a local .dev.vars.
            AWS_ACCESS_KEY_ID: '',
            AWS_SECRET_ACCESS_KEY: '',
            NEWSLETTER_FROM_EMAIL: 'News <news@example.com>',
            AWS_SES_REGION: 'us-east-1',
            NEWSLETTER_SES_CONFIG_SET: 'newsletter',
            SNS_TOPIC_ARNS: 'arn:aws:sns:us-east-1:222222222222:newsletter-events',
          },
        },
      },
    },
    testTimeout: 10000,
  },
});
