// Applies D1 migrations with cf, from a Worker's directory (its package.json db:migrate:* scripts):
//
//   bun ../scripts/d1-migrate.ts <binding> <mode> <migrations dir> [--local]
//
// cf takes the database ID rather than the binding, so this looks it up in the Worker's
// cloudflare.config.ts. --local writes to .wrangler/state, where `cf dev` keeps its local data.
import { spawnSync } from 'node:child_process';
import { relative } from 'node:path';
import { ROOT, d1DatabaseId } from './cf-config.ts';

const [binding, mode, dir, ...rest] = process.argv.slice(2);
const local = rest.includes('--local');
if (!binding || !mode || !dir || rest.some((a) => a !== '--local')) {
  console.error('Usage: bun ../scripts/d1-migrate.ts <binding> <mode> <migrations dir> [--local]');
  process.exit(1);
}

const workerDir = process.cwd();
const id = await d1DatabaseId(relative(ROOT, workerDir), mode, binding);
const args = ['d1', 'migrations', 'apply', id, '--mode', mode, '--dir', dir];
if (local) args.push('--local', '--persist-to', '.wrangler/state');

const result = spawnSync('bun', ['run', 'cf', ...args], { stdio: 'inherit' });
process.exit(result.status ?? 1);
