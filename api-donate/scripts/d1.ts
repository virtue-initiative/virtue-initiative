/** Shared by the newsletter scripts: the target environment, and running SQL with wrangler. */
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

export type Env = 'staging' | 'prod' | 'local';

const API_DONATE_DIR = fileURLToPath(new URL('..', import.meta.url));
// The local API (`api/`, through its DONATE_DB binding) is what writes the list in dev, so `local`
// reads its state rather than this worker's own.
const LOCAL_STATE_DIR = join(API_DONATE_DIR, '../api/.wrangler/state');
// Help@virtueinitiative.org's account, so wrangler needn't ask which account when a login has
// several (it can't ask, since it runs without a terminal here).
const CLOUDFLARE_ACCOUNT_ID = '5bf62eb0ef0229b690dfeb2513b5bd54';

export function fail(message: string): never {
  console.error(message);
  process.exit(1);
}

export function parseEnv(value: string | undefined, usage: string): Env {
  if (value !== 'staging' && value !== 'prod' && value !== 'local') fail(usage);
  return value;
}

export function sqlString(value: string): string {
  return `'${value.replace(/'/g, "''")}'`;
}

/**
 * Runs one SQL statement against this worker's D1 for `env` and returns its rows. Uses
 * `--command`, not `--file`: a remote `--file` goes through D1's bulk import, which returns no
 * rows and makes the database unavailable while it runs.
 */
export function d1<T>(env: Env, sql: string): T[] {
  const target =
    env === 'local'
      ? ['--env', 'staging', '--local', '--persist-to', LOCAL_STATE_DIR]
      : ['--env', env, '--remote'];
  const out = execFileSync(
    'bunx',
    ['wrangler', 'd1', 'execute', 'DB', ...target, '--json', '--command', sql],
    {
      cwd: API_DONATE_DIR,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'inherit'],
      maxBuffer: 256 * 1024 * 1024,
      env: {
        ...process.env,
        CLOUDFLARE_ACCOUNT_ID: process.env.CLOUDFLARE_ACCOUNT_ID || CLOUDFLARE_ACCOUNT_ID,
      },
    },
  );
  const results = JSON.parse(out) as { results: T[] }[];
  return results.at(-1)?.results ?? [];
}
