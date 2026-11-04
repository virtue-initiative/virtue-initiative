import { bindings, defineConfig, triggers } from 'cf/config';
import { CRONS } from './src/crons.ts';

// Select a deployment with `--mode prod` or `--mode staging`. Local dev, tests and
// seeding use staging's bindings. `--mode launch` is staging too, under its own name so
// scripts/launch.sh can hand the dev server a `.dev.vars.launch` of its own. Modes don't
// inherit from each other, so anything they share is built here once.
const MODES = {
  prod: {
    name: 'api',
    db: { name: 'app-db', id: '141a161a-a16b-41cb-a8b3-e5e40803391a' },
    donateDb: { name: 'donate-db', id: 'e5917246-2afa-445e-8aa4-9cd7d240bc11' },
    bucket: 'app-bucket',
    routes: ['api.virtueinitiative.org/*', 'app.virtueinitiative.org/api/*'],
    vars: {
      JWT_PUBLIC_KEY:
        '-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEAqscGXAACNQeQfhFi3eTH61Wy9NWAGBg+ne8KDqPHTAg=\n-----END PUBLIC KEY-----',
      R2_URL: 'https://r2.virtueinitiative.org',
      APP_URL: 'https://app.virtueinitiative.org',
      LANDING_URL: 'https://virtueinitiative.org',
      HASH_SERVER_URL: 'https://hash.virtueinitiative.org',
      SNS_TOPIC_ARNS:
        'arn:aws:sns:us-east-1:154627564714:virtue-ses-bounces,arn:aws:sns:us-east-1:154627564714:virtue-ses-complaints,arn:aws:sns:us-east-1:599243104123:virtue-ses-bounces,arn:aws:sns:us-east-1:599243104123:virtue-ses-complaints,arn:aws:sns:us-east-1:599243104123:virtue-newsletter-events',
    },
  },
  staging: {
    name: 'staging-api',
    db: { name: 'staging-app-db', id: 'f00a68e9-9ca0-4fa9-98e9-c135a489df3f' },
    donateDb: { name: 'staging-donate-db', id: '0eb0216c-c399-425b-9e60-c569f75fee02' },
    bucket: 'staging-app-bucket',
    routes: ['staging.app.virtueinitiative.org/api/*'],
    vars: {
      JWT_PUBLIC_KEY:
        '-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEARCen2jsj53qDZBVcGdZ0pe7JCiREYC3B9ni3dC0uTSY=\n-----END PUBLIC KEY-----',
      R2_URL: 'https://staging.r2.virtueinitiative.org',
      APP_URL: 'https://staging.app.virtueinitiative.org',
      LANDING_URL: 'https://staging.virtueinitiative.org',
      HASH_SERVER_URL: 'https://staging-hash.virtueinitiative.org',
      SNS_TOPIC_ARNS:
        'arn:aws:sns:us-east-1:599243104123:virtue-ses-bounces,arn:aws:sns:us-east-1:599243104123:virtue-ses-complaints,arn:aws:sns:us-east-1:599243104123:virtue-newsletter-events',
    },
  },
};

// help@virtueinitiative.org's account. CLOUDFLARE_ACCOUNT_ID, when set, wins.
const ACCOUNT_ID = '5bf62eb0ef0229b690dfeb2513b5bd54';

const SHARED_VARS = {
  JWT_ACCESS_EXPIRY: '15m',
  JWT_REFRESH_EXPIRY: '7d',
  API_BASE_PATH: '/api',
  APP_NAME: 'Virtue Initiative',
  AWS_SES_REGION: 'us-east-1',
  AWS_SES_FROM_EMAIL: 'notifications@virtueinitiative.org',
  EMAIL_DELIVERY_MODE: 'ses',
  BUG_REPORT_EMAIL: 'help@virtueinitiative.org',
};

export default defineConfig(({ mode }) => {
  if (mode !== 'prod' && mode !== 'staging' && mode !== 'launch') {
    throw new Error(`Pass --mode prod or --mode staging (got ${mode}).`);
  }
  const m = MODES[mode === 'launch' ? 'staging' : mode];
  const vars: Record<string, string> = { ...SHARED_VARS, ...m.vars };
  // Set by the deploy workflow; `GET /` reports it.
  if (process.env.COMMIT_SHA) vars.COMMIT_SHA = process.env.COMMIT_SHA;

  return {
    accountId: ACCOUNT_ID,
    worker: {
      name: m.name,
      compatibilityDate: '2024-12-01',
      compatibilityFlags: ['nodejs_compat'],
      entrypoint: 'src/index.ts',
      workersDev: false,
      observability: { logs: { enabled: true, invocationLogs: true } },
      triggers: [
        ...m.routes.map((pattern) => triggers.fetch({ pattern, zone: 'virtueinitiative.org' })),
        ...CRONS.map((schedule) => triggers.scheduled({ schedule })),
      ],
      env: {
        ...Object.fromEntries(Object.entries(vars).map(([k, v]) => [k, bindings.text(v)])),
        DB: bindings.d1(m.db),
        DONATE_DB: bindings.d1(m.donateDb),
        BUCKET: bindings.r2({ name: m.bucket }),
        RATE_LIMITER: bindings.rateLimit({ namespace: '1001', simple: { limit: 60, period: 60 } }),
      },
    },
  };
});
