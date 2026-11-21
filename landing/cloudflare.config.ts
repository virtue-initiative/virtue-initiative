import { defineConfig } from 'cf/config';

// Select a deployment with `--mode prod` or `--mode staging`. Either one serves the
// static build in dist/ (see wrangler.config.ts).
const MODES = {
  prod: { name: 'landing', domain: 'virtueinitiative.org' },
  staging: { name: 'staging-landing', domain: 'staging.virtueinitiative.org' },
};

export default defineConfig(({ mode }) => {
  if (mode !== 'prod' && mode !== 'staging') {
    throw new Error(`Pass --mode prod or --mode staging (got ${mode}).`);
  }
  const m = MODES[mode];

  return {
    // help@virtueinitiative.org's account. CLOUDFLARE_ACCOUNT_ID, when set, wins.
    accountId: '5bf62eb0ef0229b690dfeb2513b5bd54',
    worker: {
      name: m.name,
      compatibilityDate: '2026-02-23',
      workersDev: false,
      assets: { notFoundHandling: '404-page' },
      domains: [m.domain],
    },
  };
});
