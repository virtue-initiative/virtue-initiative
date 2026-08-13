// Shared helpers for the per-platform device -> api/hash-server integration
// tests: client/linux/scripts/integration-test.ts and
// client/mac/scripts/integration-test.ts.
//
// Both drive the same shape: boot the api worker locally against a fresh D1
// database (the api's own D1-backed /hash routes stand in for the standalone
// Rust hash-server in local dev -- see api/src/lib/hash-server.ts and
// scripts/launch.sh), seed the deterministic dev account, build and run the
// real platform daemon, log it in, wait briefly, then assert that hashes and
// batches actually landed in the database.

import { openSync, closeSync } from 'node:fs';
import { createServer } from 'node:net';
import type { Subprocess } from 'bun';

export const DEV_EMAIL = 'dev@dev.com';
export const DEV_PASSWORD = 'devpassword';
export const DEVICE_NAME = `ci-integration-test-${process.pid}`;

export function log(message: string): void {
  console.log(`== ${message} ==`);
}

export function fail(message: string): never {
  throw new Error(message);
}

/** Finds a free TCP port the same way the api dev server's own scripts do. */
export async function pickFreePort(): Promise<number> {
  return await new Promise((resolve, reject) => {
    const server = createServer();
    server.on('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address() as { port: number };
      server.close(() => resolve(port));
    });
  });
}

/**
 * Spawns a long-running background process with combined stdout+stderr
 * redirected to `logPath` (truncated first), mirroring `cmd > logPath 2>&1 &`.
 * The caller is responsible for killing it via `stopProcess`.
 */
export function spawnLogged(
  cmd: string[],
  opts: { cwd?: string; env?: Record<string, string>; logPath: string },
): Subprocess {
  const fd = openSync(opts.logPath, 'w');
  try {
    return Bun.spawn(cmd, {
      cwd: opts.cwd,
      env: opts.env ?? (process.env as Record<string, string>),
      stdout: fd,
      stderr: fd,
      stdin: 'ignore',
    });
  } finally {
    // Bun.spawn dup()s the fd for the child; our copy can close immediately.
    closeSync(fd);
  }
}

/** Sends SIGTERM and waits for exit, swallowing errors (process may already be gone). */
export async function stopProcess(proc: Subprocess | undefined): Promise<void> {
  if (!proc) return;
  try {
    proc.kill();
    await proc.exited;
  } catch {
    // already exited
  }
}

export function sleep(seconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, seconds * 1000));
}

/** Polls `condition` once a second until it returns true or `tries` is exhausted. */
export async function waitUntil(
  tries: number,
  condition: () => boolean | Promise<boolean>,
): Promise<boolean> {
  for (let i = 0; i < tries; i++) {
    if (await condition()) return true;
    await sleep(1);
  }
  return false;
}

export async function waitForHttpReady(url: string, tries = 60): Promise<void> {
  log('Waiting for api dev server to become ready');
  const ready = await waitUntil(tries, async () => {
    try {
      const res = await fetch(url);
      return res.ok || res.status < 500;
    } catch {
      return false;
    }
  });
  if (!ready) fail('api dev server did not become ready in time');
}

export async function setupApiDevEnvironment(apiDir: string): Promise<void> {
  log('Setting up api/ local dev environment');
  const devVarsExample = `${apiDir}/.dev.vars.example`;
  const devVars = `${apiDir}/.dev.vars`;
  if (!(await Bun.file(devVars).exists())) {
    await Bun.write(devVars, await Bun.file(devVarsExample).text());
  }
  if (!(await Bun.file(`${apiDir}/node_modules/.bin/wrangler`).exists())) {
    run(['bun', 'install'], { cwd: apiDir });
  }
  run(['bun', 'run', 'db:migrate:local'], { cwd: apiDir });
}

export function startApiDevServer(
  apiDir: string,
  apiPort: number,
  apiBaseUrl: string,
  logPath: string,
): Subprocess {
  log('Starting api dev server');
  return spawnLogged(
    [
      'bun',
      'run',
      'dev',
      '--',
      '--port',
      String(apiPort),
      '--var',
      `HASH_SERVER_URL:${apiBaseUrl}/api`,
    ],
    { cwd: apiDir, logPath },
  );
}

export function seedDevUser(rootDir: string): void {
  log('Seeding dev user');
  run(['bun', 'run', `${rootDir}/scripts/seed-dev-user.mjs`]);
}

/** Runs a foreground command with inherited stdio, throwing on non-zero exit. */
export function run(
  cmd: string[],
  opts: { cwd?: string; env?: Record<string, string> } = {},
): void {
  const result = Bun.spawnSync(cmd, {
    cwd: opts.cwd,
    env: opts.env ?? (process.env as Record<string, string>),
    stdout: 'inherit',
    stderr: 'inherit',
  });
  if (!result.success) {
    fail(`command failed (exit ${result.exitCode}): ${cmd.join(' ')}`);
  }
}

/** `wrangler d1 execute --json`, parsed directly -- no shell-out needed to extract the count. */
function d1QueryCount(apiDir: string, sql: string): number {
  const result = Bun.spawnSync(
    [
      'bun',
      'run',
      'wrangler',
      'd1',
      'execute',
      'staging-app-db',
      '--local',
      '--env',
      'staging',
      '--json',
      '--command',
      sql,
    ],
    { cwd: apiDir, stdout: 'pipe', stderr: 'inherit' },
  );
  if (!result.success) fail(`wrangler d1 execute failed for: ${sql}`);
  const data = JSON.parse(result.stdout.toString());
  return data[0]?.results?.[0]?.c ?? 0;
}

function d1Dump(apiDir: string, label: string, sql: string): void {
  console.log(`--- ${label} (raw) ---`);
  run(
    [
      'bun',
      'run',
      'wrangler',
      'd1',
      'execute',
      'staging-app-db',
      '--local',
      '--env',
      'staging',
      '--command',
      sql,
    ],
    {
      cwd: apiDir,
    },
  );
}

/**
 * Asserts a device/hash/batch row landed for `deviceName`, retrying for a few
 * seconds since a `wrangler d1 execute --local` CLI process reading the same
 * on-disk D1 state can lag slightly behind Miniflare's in-process view right
 * after a write.
 *
 * hash_states.count is a rolling per-batch-window counter, not a cumulative
 * total: api/src/routes/device-only.ts resets it to 0 after every successful
 * POST /d/batch (see hashReset() there), so with a short batch window it can
 * legitimately read 0 moments after a hash was ingested. hashed_at is never
 * touched by that reset (see localHashReset in api/src/lib/hash-server.ts),
 * so it's the durable signal that at least one hash was ever ingested.
 */
export async function verifyDeviceHashBatch(
  apiDir: string,
  deviceName: string,
  tries = 15,
): Promise<void> {
  log('Verifying database state');

  let deviceCount = 0;
  let hashCount = 0;
  let batchCount = 0;
  let ok = false;

  for (let i = 0; i < tries; i++) {
    deviceCount = d1QueryCount(
      apiDir,
      `SELECT COUNT(*) as c FROM devices WHERE name = '${deviceName}'`,
    );
    hashCount = d1QueryCount(
      apiDir,
      `SELECT COUNT(*) as c FROM hash_states hs JOIN devices d ON d.id = hs.device_id WHERE d.name = '${deviceName}' AND hs.hashed_at IS NOT NULL`,
    );
    batchCount = d1QueryCount(
      apiDir,
      `SELECT COUNT(*) as c FROM batches b JOIN devices d ON d.id = b.device_id WHERE d.name = '${deviceName}'`,
    );
    console.log(
      `device rows: ${deviceCount}, ever-hashed: ${hashCount}, batch rows: ${batchCount}`,
    );
    ok = deviceCount >= 1 && hashCount >= 1 && batchCount >= 1;
    if (ok) break;
    await sleep(2);
  }

  if (ok) return;

  if (deviceCount < 1)
    console.error(`integration-test: expected a devices row for '${deviceName}'`);
  if (hashCount < 1)
    console.error(
      `integration-test: expected a hash_states row with hashed_at set for '${deviceName}'`,
    );
  if (batchCount < 1)
    console.error(`integration-test: expected at least one batch row for '${deviceName}'`);

  d1Dump(apiDir, 'devices', 'SELECT hex(id) as id, name FROM devices');
  d1Dump(
    apiDir,
    'hash_states',
    'SELECT hex(device_id) as device_id, count, hashed_at FROM hash_states',
  );
  d1Dump(
    apiDir,
    'batches',
    'SELECT hex(device_id) as device_id, COUNT(*) as n FROM batches GROUP BY device_id',
  );

  fail('device/hash/batch rows did not land in time');
}

export function requireCommands(cmds: string[]): void {
  for (const cmd of cmds) {
    const found = Bun.spawnSync(['which', cmd], { stdout: 'ignore', stderr: 'ignore' }).success;
    if (!found) fail(`missing required command '${cmd}' on PATH`);
  }
}

/** Runs `main`, printing captured logs and exiting 1 on failure; exits 0 on success. */
export async function runIntegrationTest(
  main: () => Promise<void>,
  dumpLogsOnFailure: () => void | Promise<void>,
): Promise<void> {
  try {
    await main();
    log('Integration test passed');
  } catch (err) {
    console.error(`integration-test: ${err instanceof Error ? err.message : err}`);
    await dumpLogsOnFailure();
    process.exitCode = 1;
  }
}
