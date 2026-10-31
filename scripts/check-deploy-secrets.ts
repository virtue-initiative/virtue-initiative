#!/usr/bin/env bun
// Checks that every Worker a push is about to deploy already has its required secrets, so a
// missing one stops the release before anything ships. Cloudflare only returns the names, so a
// secret set to an empty value still passes.
//
//   bun scripts/check-deploy-secrets.ts staging        # or main, the branch being deployed
//   bun scripts/check-deploy-secrets.ts --check-usage  # PR check, needs no credentials
//
// --check-usage fails when a Worker's code uses a value that no wrangler.json and no list below
// accounts for, which is what a new secret looks like until it is added here.
import { spawnSync } from 'node:child_process';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const ROOT = join(import.meta.dir, '..');

// Each Worker's `Env` points back here. The `web` and `landing` Workers only serve assets and
// have no secrets.
const REQUIRED: Record<string, string[]> = {
  api: [
    'AWS_ACCESS_KEY_ID',
    'AWS_SECRET_ACCESS_KEY',
    'JWT_PRIVATE_KEY',
    'LOGIN_SALT_SECRET',
    'TURNSTILE_SECRET_KEY',
  ],
  'api-donate': ['STRIPE_SECRET_KEY', 'STRIPE_WEBHOOK_SECRET'],
};

// In `Env` but never deployed as a secret: the deploy passes COMMIT_SHA with --var, and only
// local dev sets DEV_R2_ROUTE.
const NOT_SECRETS = ['COMMIT_SHA', 'DEV_R2_ROUTE'];

type Target = { project: string; env: string };

// What .github/workflows/deploy.yml deploys for each branch.
const TARGETS: Record<string, Target[]> = {
  main: [
    { project: 'api', env: 'prod' },
    { project: 'api-donate', env: 'prod' },
  ],
  staging: [
    { project: 'api', env: 'staging' },
    { project: 'api-donate', env: 'staging' },
    // The donate worker has no separate prod release, so staging pushes ship to prod too.
    { project: 'api-donate', env: 'prod' },
  ],
};

/** Notices can come before the JSON, even ones that start with "[". */
function parseSecretNames(stdout: string): Set<string> {
  const lines = stdout.split('\n');
  for (const [i, line] of lines.entries()) {
    if (!line.startsWith('[')) continue;
    try {
      const secrets = JSON.parse(lines.slice(i).join('\n')) as { name: string }[];
      return new Set(secrets.map((secret) => secret.name));
    } catch {
      // A notice, so keep looking.
    }
  }
  throw new Error(`no secret list in wrangler's output:\n${stdout}`);
}

function listSecrets({ project, env }: Target): Set<string> {
  const res = spawnSync('bunx', ['wrangler', 'secret', 'list', '--env', env, '--format', 'json'], {
    cwd: join(ROOT, project),
    encoding: 'utf8',
  });
  if (res.status !== 0) {
    throw new Error(res.stderr || res.stdout || `wrangler exited with ${res.status}`);
  }
  return parseSecretNames(res.stdout);
}

/** Every value the Worker reads from `env` or declares in its `Env` interface. */
function usedNames(project: string): Set<string> {
  const src = join(ROOT, project, 'src');
  const names = new Set<string>();
  for (const file of readdirSync(src, { recursive: true }) as string[]) {
    if (!file.endsWith('.ts')) continue;
    const code = readFileSync(join(src, file), 'utf8');
    for (const match of code.matchAll(/\benv\.([A-Z][A-Z0-9_]*)/g)) names.add(match[1]!);
  }
  const bindings = readFileSync(join(src, 'types/bindings.ts'), 'utf8');
  const env = bindings.slice(
    bindings.indexOf('interface Env'),
    bindings.indexOf('interface Variables'),
  );
  for (const match of env.matchAll(/^\s*([A-Z][A-Z0-9_]*)\??:/gm)) names.add(match[1]!);
  return names;
}

function checkUsage(): boolean {
  let ok = true;
  for (const [project, required] of Object.entries(REQUIRED)) {
    const used = usedNames(project);
    for (const name of required.filter((name) => !used.has(name))) {
      console.error(
        `${project} no longer uses ${name}. Remove it from scripts/check-deploy-secrets.ts.`,
      );
      ok = false;
    }
    const config = JSON.parse(readFileSync(join(ROOT, project, 'wrangler.json'), 'utf8'));
    for (const [env, settings] of Object.entries<Record<string, any>>(config.env)) {
      const known = new Set([
        ...required,
        ...NOT_SECRETS,
        ...Object.keys(settings.vars ?? {}),
        ...(settings.d1_databases ?? []).map((binding: { binding: string }) => binding.binding),
        ...(settings.r2_buckets ?? []).map((binding: { binding: string }) => binding.binding),
        ...(settings.ratelimits ?? []).map((binding: { name: string }) => binding.name),
      ]);
      for (const name of [...used].filter((name) => !known.has(name))) {
        console.error(
          `${project} (${env}) uses ${name}, which is not in wrangler.json. If it is a secret, add it to scripts/check-deploy-secrets.ts.`,
        );
        ok = false;
      }
    }
  }
  return ok;
}

if (process.argv[2] === '--check-usage') {
  const ok = checkUsage();
  if (ok) console.log('Every value the Workers use is accounted for.');
  process.exit(ok ? 0 : 1);
}

const targets = TARGETS[process.argv[2] ?? ''];
if (!targets) {
  console.error(
    `Usage: bun scripts/check-deploy-secrets.ts <${Object.keys(TARGETS).join('|')}|--check-usage>`,
  );
  process.exit(2);
}

let ok = true;
for (const target of targets) {
  const label = `${target.project} (${target.env})`;
  let existing: Set<string>;
  try {
    existing = listSecrets(target);
  } catch (err) {
    // Not the same as a missing secret, so the fix below would mislead.
    console.error(`Could not list the secrets of ${label}: ${err}`);
    ok = false;
    continue;
  }
  const required = REQUIRED[target.project]!;
  const missing = required.filter((name) => !existing.has(name));
  for (const name of missing) {
    console.error(
      `${label} is missing ${name}. Set it with: cd ${target.project} && bunx wrangler secret put ${name} --env ${target.env}`,
    );
  }
  if (missing.length > 0) ok = false;
  else console.log(`${label} has all ${required.length} required secrets.`);
}
process.exit(ok ? 0 : 1);
