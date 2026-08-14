#!/usr/bin/env bun
// Device -> api/hash-server integration smoke test (Linux).
//
// Boots the api worker locally alongside a real standalone hash-server
// process (see hash-server/SPEC.md and scripts/launch.sh, which starts it
// the same way for local dev), seeds the deterministic dev account, builds
// and runs the real virtue-linux daemon under Xvfb (so screenshot capture
// produces a genuine, if black, screenshot with no mocking code), logs in,
// lets it run for a short window, then asserts that a device/batch landed
// in D1 and that the hash server actually ingested a hash for it.
//
// Usage: bun client/linux/scripts/integration-test.ts
//
// Requires: bun, cargo, curl, xvfb-run, all on PATH.

import { mkdtempSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { Subprocess } from 'bun';
import { ciLogin } from './ci-login.ts';
import {
  DEV_EMAIL,
  DEV_PASSWORD,
  DEVICE_NAME,
  fail,
  log,
  pickFreePort,
  readDevVar,
  requireCommands,
  run,
  runIntegrationTest,
  seedDevUser,
  setupApiDevEnvironment,
  spawnLogged,
  startApiDevServer,
  startHashServer,
  stopProcess,
  verifyDeviceHashBatch,
  waitForHttpReady,
  waitUntil,
} from '../../scripts/integration-test-lib.ts';

const SCRIPT_DIR = import.meta.dir;
const ROOT = join(SCRIPT_DIR, '../../..');
const CLIENT_DIR = join(ROOT, 'client');
const API_DIR = join(ROOT, 'api');
const HASH_SERVER_DIR = join(ROOT, 'hash-server');

// capture_interval_seconds has a 15s floor enforced by client/core/src/config.rs.
const CAPTURE_INTERVAL_SECONDS = 15;
const BATCH_WINDOW_SECONDS = 15;
const RUN_DURATION_SECONDS = 60;

requireCommands(['bun', 'cargo', 'curl', 'xvfb-run']);

const logDir = mkdtempSync(join(tmpdir(), 'virtue-linux-ci-log-'));
const apiLog = join(logDir, 'api.log');
const hashLog = join(logDir, 'hash-server.log');
const daemonLog = join(logDir, 'daemon.log');

// Isolated home for the client under test only -- NOT exported globally.
// rustup resolves its default toolchain from $HOME/.rustup at runtime (unlike
// $CARGO_HOME, which the Setup Rust step pins explicitly), so swapping HOME
// for the whole process would break `cargo build`. Only the daemon and login
// processes below get this HOME/XDG override.
const tmpHome = mkdtempSync(join(tmpdir(), 'virtue-linux-ci-home-'));
const xdgConfigHome = join(tmpHome, 'config');
const xdgStateHome = join(tmpHome, 'state');
const clientEnv: Record<string, string> = {
  ...(process.env as Record<string, string>),
  HOME: tmpHome,
  XDG_CONFIG_HOME: xdgConfigHome,
  XDG_STATE_HOME: xdgStateHome,
};

let apiProc: Subprocess | undefined;
let hashProc: Subprocess | undefined;
let daemonProc: Subprocess | undefined;
let virtueBin = '';

async function cleanup(): Promise<void> {
  if (daemonProc) {
    await stopProcess(daemonProc);
    // xvfb-run doesn't reliably forward TERM to the command it wraps, so the
    // daemon (and its Xvfb server) can otherwise survive as an orphan.
    Bun.spawnSync(['pkill', '-f', 'Xvfb :']);
    if (virtueBin) Bun.spawnSync(['pkill', '-f', `${virtueBin} daemon`]);
  }
  await stopProcess(apiProc);
  await stopProcess(hashProc);
  rmSync(tmpHome, { recursive: true, force: true });
  rmSync(logDir, { recursive: true, force: true });
}

async function dumpLogs(): Promise<void> {
  console.log('=== api log ===');
  if (existsSync(apiLog)) console.log(await Bun.file(apiLog).text());
  console.log('=== hash server log ===');
  if (existsSync(hashLog)) console.log(await Bun.file(hashLog).text());
  console.log('=== daemon log ===');
  if (existsSync(daemonLog)) console.log(await Bun.file(daemonLog).text());
}

for (const sig of ['SIGINT', 'SIGTERM'] as const) {
  process.on(sig, async () => {
    await cleanup();
    process.exit(1);
  });
}

async function main(): Promise<void> {
  const apiPort = await pickFreePort();
  const hashPort = await pickFreePort();
  const apiBaseUrl = `http://localhost:${apiPort}`;
  const hashBaseUrl = `http://localhost:${hashPort}`;

  await setupApiDevEnvironment(API_DIR);
  const jwtPublicKeyPem = readDevVar(API_DIR, 'JWT_PUBLIC_KEY');

  hashProc = startHashServer(
    HASH_SERVER_DIR,
    hashPort,
    jwtPublicKeyPem,
    join(logDir, 'hash-server.sqlite'),
    hashLog,
  );
  apiProc = startApiDevServer(API_DIR, apiPort, hashBaseUrl, apiLog);
  await waitForHttpReady(`${hashBaseUrl}/`, 'hash server');
  await waitForHttpReady(`${apiBaseUrl}/`, 'api dev server');

  seedDevUser(ROOT);

  log('Building virtue-linux client');
  run(['cargo', 'build', '-p', 'virtue-linux'], {
    cwd: CLIENT_DIR,
    env: {
      ...(process.env as Record<string, string>),
      VIRTUE_DEFAULT_API_URL: apiBaseUrl,
      VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS: String(CAPTURE_INTERVAL_SECONDS),
      VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS: String(BATCH_WINDOW_SECONDS),
    },
  });
  virtueBin = join(CLIENT_DIR, 'target/debug/virtue');

  log('Starting the daemon under Xvfb');
  daemonProc = spawnLogged(['xvfb-run', '-a', virtueBin, 'daemon'], {
    env: clientEnv,
    logPath: daemonLog,
  });

  log('Waiting for the daemon IPC socket');
  const daemonSock = join(xdgStateHome, 'virtue/daemon.sock');
  const daemonReady = await waitUntil(30, () => existsSync(daemonSock));
  if (!daemonReady) fail('daemon did not create its IPC socket in time');

  log('Logging in');
  await ciLogin({
    bin: virtueBin,
    email: DEV_EMAIL,
    password: DEV_PASSWORD,
    deviceName: DEVICE_NAME,
    env: clientEnv,
  });

  log(`Waiting ${RUN_DURATION_SECONDS}s for capture/batch/hash activity`);
  await new Promise((resolve) => setTimeout(resolve, RUN_DURATION_SECONDS * 1000));

  const jwtPrivateKeyPem = readDevVar(API_DIR, 'JWT_PRIVATE_KEY');
  await verifyDeviceHashBatch(API_DIR, HASH_SERVER_DIR, hashBaseUrl, jwtPrivateKeyPem, DEVICE_NAME);
}

await runIntegrationTest(main, dumpLogs);
await cleanup();
process.exit(process.exitCode ?? 0);
