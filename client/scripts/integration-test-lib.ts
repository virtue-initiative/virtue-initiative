// Shared helpers for the per-platform device -> api/hash-server integration
// tests: client/linux/scripts/integration-test.ts, client/mac/scripts/
// integration-test.ts, and client/windows/scripts/integration-test.ts.
//
// All three drive the same shape: boot the api worker locally alongside a
// real standalone hash-server process (see hash-server/SPEC.md and
// scripts/launch.sh, which starts it the same way for local dev), seed the
// deterministic dev account, build and run the real platform daemon, log it
// in, wait briefly, then assert that a device/batch landed in D1 and that
// the hash server actually ingested a hash for it.

import { openSync, closeSync, readFileSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { createServer } from 'node:net';
import { join } from 'node:path';
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

export async function waitForHttpReady(url: string, label: string, tries = 60): Promise<void> {
  log(`Waiting for ${label} to become ready`);
  const ready = await waitUntil(tries, async () => {
    try {
      const res = await fetch(url);
      return res.ok || res.status < 500;
    } catch {
      return false;
    }
  });
  if (!ready) fail(`${label} did not become ready in time`);
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

/** Reads and un-escapes a `NAME="..."` value from `apiDir`'s `.dev.vars` (see api/.dev.vars.example). */
export function readDevVar(apiDir: string, name: string): string {
  const contents = readFileSync(join(apiDir, '.dev.vars'), 'utf8');
  const match = contents.match(new RegExp(`^${name}="(.*)"$`, 'm'));
  if (!match) fail(`integration-test: ${name} not found in ${apiDir}/.dev.vars`);
  return match[1].replace(/\\n/g, '\n');
}

/** Starts the real standalone hash server (hash-server/), the same way scripts/launch.sh does for local dev. */
export function startHashServer(
  hashServerDir: string,
  hashPort: number,
  jwtPublicKeyPem: string,
  databasePath: string,
  logPath: string,
): Subprocess {
  log('Starting hash server');
  return spawnLogged(['cargo', 'run', '--quiet'], {
    cwd: hashServerDir,
    env: {
      ...(process.env as Record<string, string>),
      HOST: '127.0.0.1',
      PORT: String(hashPort),
      JWT_PUBLIC_KEY: jwtPublicKeyPem,
      DATABASE_PATH: databasePath,
      RUST_LOG: 'info',
    },
    logPath,
  });
}

export function startApiDevServer(
  apiDir: string,
  apiPort: number,
  hashBaseUrl: string,
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
      `HASH_SERVER_URL:${hashBaseUrl}`,
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

/** `wrangler d1 execute --json`, parsed directly -- no shell-out needed to extract fields. */
function d1QueryJson(
  apiDir: string,
  sql: string,
): Array<{ results?: Array<Record<string, unknown>> }> {
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
  return JSON.parse(result.stdout.toString());
}

function d1QueryCount(apiDir: string, sql: string): number {
  return (d1QueryJson(apiDir, sql)[0]?.results?.[0]?.c as number | undefined) ?? 0;
}

function d1QueryValue(apiDir: string, sql: string): string {
  return (d1QueryJson(apiDir, sql)[0]?.results?.[0]?.v as string | undefined) ?? '';
}

/** `hex(id)` from D1 (big-endian, no dashes, uppercase) back to a standard lowercase UUID string. */
function hexToUuid(hex: string): string {
  const h = hex.toLowerCase();
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20, 32)}`;
}

/** Mints a `server`-typed JWT via hash-server's own dev/test helper (see hash-server/SPEC.md section 4.1). */
function mintServerToken(hashServerDir: string, privateKeyPemPath: string): string {
  const result = Bun.spawnSync(
    [
      'cargo',
      'run',
      '--quiet',
      '--example',
      'mint_token',
      '--',
      'ci-integration-test',
      'server',
      privateKeyPemPath,
    ],
    { cwd: hashServerDir, stdout: 'pipe', stderr: 'inherit' },
  );
  if (!result.success) fail('failed to mint a hash-server server token');
  return result.stdout.toString().trim();
}

/** `GET /hash?devices=<id>` on the standalone hash server, returning `last_received` (0 if unknown). */
async function hashServerLastReceived(
  hashBaseUrl: string,
  serverToken: string,
  deviceId: string,
): Promise<number> {
  const resp = await fetch(`${hashBaseUrl}/hash?devices=${deviceId}`, {
    headers: { Authorization: `Bearer ${serverToken}` },
  });
  if (!resp.ok) fail(`hash server GET /hash failed: ${resp.status}`);
  const body = (await resp.json()) as Record<string, { last_received: number }>;
  return body[deviceId]?.last_received ?? 0;
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
 * Asserts a device row and at least one batch row landed in D1, and that the
 * standalone hash server actually ingested a hash for that device -- all
 * retried for a few seconds since a `wrangler d1 execute --local` CLI process
 * reading the same on-disk D1 state can lag slightly behind Miniflare's
 * in-process view right after a write.
 *
 * Hash-chain state itself no longer lives in D1 (see hash-server/SPEC.md) --
 * it's checked directly against the hash server via `GET /hash?devices=<id>`,
 * authenticated with a `server` token minted the same way the api does (see
 * hash-server/examples/mint_token.rs). `last_received` is never cleared by
 * POST /d/batch's hashReset() (SPEC.md section 2.3 -- reset zeroes the hash
 * and seq but not last_received), so it's the durable signal that at least
 * one hash was ever ingested, mirroring the old D1 `hashed_at` check.
 */
export async function verifyDeviceHashBatch(
  apiDir: string,
  hashServerDir: string,
  hashBaseUrl: string,
  jwtPrivateKeyPem: string,
  deviceName: string,
  tries = 15,
): Promise<void> {
  log('Verifying D1 and hash-server state');

  const keyDir = mkdtempSync(join(tmpdir(), 'virtue-ci-jwt-'));
  let serverToken: string;
  try {
    const keyPath = join(keyDir, 'jwt-private-key.pem');
    writeFileSync(keyPath, jwtPrivateKeyPem);
    serverToken = mintServerToken(hashServerDir, keyPath);
  } finally {
    rmSync(keyDir, { recursive: true, force: true });
  }

  let deviceCount = 0;
  let batchCount = 0;
  let lastReceived = 0;
  let ok = false;

  for (let i = 0; i < tries; i++) {
    deviceCount = d1QueryCount(
      apiDir,
      `SELECT COUNT(*) as c FROM devices WHERE name = '${deviceName}'`,
    );
    const deviceIdHex =
      deviceCount >= 1
        ? d1QueryValue(apiDir, `SELECT hex(id) as v FROM devices WHERE name = '${deviceName}'`)
        : '';
    batchCount = d1QueryCount(
      apiDir,
      `SELECT COUNT(*) as c FROM batches b JOIN devices d ON d.id = b.device_id WHERE d.name = '${deviceName}'`,
    );
    lastReceived = deviceIdHex
      ? await hashServerLastReceived(hashBaseUrl, serverToken, hexToUuid(deviceIdHex))
      : 0;

    console.log(
      `device rows: ${deviceCount}, batch rows: ${batchCount}, hash-server last_received: ${lastReceived}`,
    );
    ok = deviceCount >= 1 && batchCount >= 1 && lastReceived > 0;
    if (ok) break;
    await sleep(2);
  }

  if (ok) return;

  if (deviceCount < 1)
    console.error(`integration-test: expected a devices row for '${deviceName}'`);
  if (batchCount < 1)
    console.error(`integration-test: expected at least one batch row for '${deviceName}'`);
  if (lastReceived <= 0)
    console.error(
      `integration-test: expected the hash server to report last_received > 0 for '${deviceName}'`,
    );

  d1Dump(apiDir, 'devices', 'SELECT hex(id) as id, name FROM devices');
  d1Dump(
    apiDir,
    'batches',
    'SELECT hex(device_id) as device_id, COUNT(*) as n FROM batches GROUP BY device_id',
  );

  fail('device/batch rows or hash-server state did not land in time');
}

/** Exits 1 immediately (there's nothing to clean up yet) if any command is missing. */
export function requireCommands(cmds: string[]): void {
  for (const cmd of cmds) {
    const found = Bun.spawnSync(['which', cmd], { stdout: 'ignore', stderr: 'ignore' }).success;
    if (!found) {
      console.error(`integration-test: missing required command '${cmd}' on PATH`);
      process.exit(1);
    }
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
