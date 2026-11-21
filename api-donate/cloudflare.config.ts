import { bindings, defineConfig } from 'cf/config';

// Select a deployment with `--mode prod` or `--mode staging`. Local dev and tests use
// staging's bindings. `--mode launch` is staging too, under its own name so
// scripts/launch.sh can hand the dev server a `.dev.vars.launch` of its own.
const MODES = {
  prod: {
    name: 'donate',
    db: { name: 'donate-db', id: 'e5917246-2afa-445e-8aa4-9cd7d240bc11' },
    domain: 'donate-api.virtueinitiative.org',
    landingUrl: 'https://virtueinitiative.org',
  },
  staging: {
    name: 'staging-donate',
    db: { name: 'staging-donate-db', id: '0eb0216c-c399-425b-9e60-c569f75fee02' },
    domain: 'staging.donate-api.virtueinitiative.org',
    landingUrl: 'https://staging.virtueinitiative.org',
  },
};

export default defineConfig(({ mode }) => {
  if (mode !== 'prod' && mode !== 'staging' && mode !== 'launch') {
    throw new Error(`Pass --mode prod or --mode staging (got ${mode}).`);
  }
  const m = MODES[mode === 'launch' ? 'staging' : mode];

  return {
    // help@virtueinitiative.org's account. CLOUDFLARE_ACCOUNT_ID, when set, wins.
    accountId: '5bf62eb0ef0229b690dfeb2513b5bd54',
    worker: {
      name: m.name,
      compatibilityDate: '2024-12-01',
      compatibilityFlags: ['nodejs_compat'],
      entrypoint: 'src/index.ts',
      workersDev: false,
      observability: { logs: { enabled: true, invocationLogs: true } },
      domains: [m.domain],
      env: {
        LANDING_URL: bindings.text(m.landingUrl),
        DB: bindings.d1(m.db),
      },
    },
  };
});
