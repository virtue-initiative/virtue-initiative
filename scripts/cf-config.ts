// Reads a Worker's cloudflare.config.ts for one mode (`prod`, `staging` or `launch`), and
// runs SQL against its D1 databases with cf.
//
// cf reads that file itself. Everything else that needs a Worker's bindings (the Vitest
// pool, the seed script's platform proxy, the deploy-secrets check, the D1 scripts) gets
// them through here, converted to Wrangler's config shape, so cloudflare.config.ts stays
// the only place they are written down.
import { convertToWranglerConfig, resolveAndParseConfig } from '@cloudflare/config';
import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');

export type WranglerConfig = ReturnType<typeof convertToWranglerConfig>;

export async function readWorkerConfig(worker: string, mode: string): Promise<WranglerConfig> {
  const dir = join(ROOT, worker);
  const { default: input } = await import(pathToFileURL(join(dir, 'cloudflare.config.ts')).href);
  const parsed = await resolveAndParseConfig(input, { mode, isPreview: false });
  if (!parsed.success) {
    throw new Error(`${worker}/cloudflare.config.ts (--mode ${mode}): ${parsed.error.message}`);
  }
  const config = convertToWranglerConfig(parsed.data);
  if (config.main) config.main = resolve(dir, config.main);
  return config;
}

/**
 * Writes readWorkerConfig's result to `<worker>/.cloudflare/wrangler.<mode>.json`, for
 * tools that only take a config file path, and returns that path.
 */
export async function writeWranglerConfig(worker: string, mode: string): Promise<string> {
  const path = join(ROOT, worker, '.cloudflare', `wrangler.${mode}.json`);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify(await readWorkerConfig(worker, mode), null, 2));
  return path;
}

export type D1RawResult =
  | { ok: true; results: Record<string, unknown>[][]; stderr: string }
  | { ok: false; output: string };

/**
 * Runs `sql` with `cf d1 raw` from `worker`'s directory and returns each statement's rows as
 * objects. `localState` is a state directory (like `api/.wrangler/state`) to use instead of the
 * remote database; cf has no local `d1 query`, only `d1 raw`.
 */
export function d1Raw(
  worker: string,
  databaseId: string,
  mode: string,
  sql: string,
  localState?: string,
): D1RawResult {
  const args = ['d1', 'raw', databaseId, '--mode', mode, '--sql', sql];
  if (localState) args.push('--local', '--persist-to', localState);
  const result = spawnSync('bun', ['run', 'cf', ...args], {
    cwd: join(ROOT, worker),
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
  });
  if (result.status !== 0) return { ok: false, output: result.stderr + result.stdout };
  const statements = JSON.parse(result.stdout) as {
    results: { columns: string[]; rows: unknown[][] };
  }[];
  return {
    ok: true,
    results: statements.map(({ results: { columns, rows } }) =>
      rows.map((row) => Object.fromEntries(columns.map((c, i) => [c, row[i]]))),
    ),
    stderr: result.stderr,
  };
}

/** The ID of the D1 database bound as `binding`, which is what cf's d1 commands take. */
export async function d1DatabaseId(worker: string, mode: string, binding: string): Promise<string> {
  const config = await readWorkerConfig(worker, mode);
  const db = config.d1_databases?.find((d) => d.binding === binding);
  if (!db?.database_id) throw new Error(`${worker} has no D1 binding ${binding} in --mode ${mode}`);
  return db.database_id;
}
