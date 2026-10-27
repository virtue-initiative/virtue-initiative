/** Shared by the newsletter scripts: the target environment, and running SQL with wrangler. */
import { execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

export type Env = 'staging' | 'prod' | 'local';

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
 * Runs SQL against this worker's D1 for `env` and returns the rows of the last statement.
 * `local` is the local D1 that `bun run dev` uses.
 */
export function d1<T>(env: Env, sql: string): T[] {
  const target = env === 'local' ? ['--env', 'staging', '--local'] : ['--env', env, '--remote'];
  const sqlFile = join(tmpdir(), `newsletter-${process.pid}.sql`);
  writeFileSync(sqlFile, sql);
  const out = execFileSync(
    'bunx',
    ['wrangler', 'd1', 'execute', 'DB', ...target, '--json', '--file', sqlFile],
    { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'], maxBuffer: 256 * 1024 * 1024 },
  );
  const results = JSON.parse(out) as { results: T[] }[];
  return results.at(-1)?.results ?? [];
}
