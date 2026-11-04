/** Shared by the newsletter scripts: the target environment, and running SQL with cf. */
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { d1DatabaseId, d1Raw } from '../../scripts/cf-config';

export type Env = 'staging' | 'prod' | 'local';

const API_DONATE_DIR = fileURLToPath(new URL('..', import.meta.url));
// The local API (`api/`, through its DONATE_DB binding) is what writes the list in dev, so `local`
// reads its state rather than this worker's own.
const LOCAL_STATE_DIR = join(API_DONATE_DIR, '../api/.wrangler/state');
const DB_IDS = {
  staging: await d1DatabaseId('api-donate', 'staging', 'DB'),
  prod: await d1DatabaseId('api-donate', 'prod', 'DB'),
};

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
 * Runs one SQL statement against this worker's D1 for `env` and returns its rows. Sends it as a
 * query rather than a file: D1 imports a remote file in bulk, which returns no rows and makes the
 * database unavailable while it runs.
 */
export function d1<T>(env: Env, sql: string): T[] {
  const result =
    env === 'local'
      ? d1Raw('api-donate', DB_IDS.staging, 'staging', sql, LOCAL_STATE_DIR)
      : d1Raw('api-donate', DB_IDS[env], env, sql);
  if (!result.ok) fail(result.output);
  return (result.results.at(-1) ?? []) as T[];
}
