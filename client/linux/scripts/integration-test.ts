#!/usr/bin/env bun
// Device -> api/hash-server integration smoke test (Linux).
//
// Boots the api worker locally against a fresh D1 database (the api's own
// D1-backed /hash routes stand in for the standalone Rust hash-server in
// local dev -- see api/src/lib/hash-server.ts and scripts/launch.sh), seeds
// the deterministic dev account, builds and runs the real virtue-linux
// daemon under Xvfb (so screenshot capture produces a genuine, if black,
// screenshot with no mocking code), logs in, lets it run for a short window,
// then asserts that hashes and batches actually landed in the database.
//
// Usage: bun client/linux/scripts/integration-test.ts
//
// Requires: bun, cargo, curl, xvfb-run, all on PATH.

import { mkdirSync, mkdtempSync, existsSync, rmSync } from 'node:fs';
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
  requireCommands,
  run,
  runIntegrationTest,
  seedDevUser,
  setupApiDevEnvironment,
  spawnLogged,
  startApiDevServer,
  stopProcess,
  verifyDeviceHashBatch,
  waitForHttpReady,
  waitUntil,
} from '../../scripts/integration-test-lib.ts';

const SCRIPT_DIR = import.meta.dir;
const ROOT = join(SCRIPT_DIR, '../../..');
const CLIENT_DIR = join(ROOT, 'client');
const API_DIR = join(ROOT, 'api');

// capture_interval_seconds has a 15s floor enforced by client/core/src/config.rs.
const CAPTURE_INTERVAL_SECONDS = 15;
const BATCH_WINDOW_SECONDS = 15;
const RUN_DURATION_SECONDS = 60;

requireCommands(['bun', 'cargo', 'curl', 'xvfb-run']);

const logDir = mkdtempSync(join(tmpdir(), 'virtue-linux-ci-log-'));
const apiLog = join(logDir, 'api.log');
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
  rmSync(tmpHome, { recursive: true, force: true });
  rmSync(logDir, { recursive: true, force: true });
}

async function dumpLogs(): Promise<void> {
  console.log('=== api log ===');
  if (existsSync(apiLog)) console.log(await Bun.file(apiLog).text());
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
  const apiBaseUrl = `http://localhost:${apiPort}`;

  await setupApiDevEnvironment(API_DIR);

  apiProc = startApiDevServer(API_DIR, apiPort, apiBaseUrl, apiLog);
  await waitForHttpReady(`${apiBaseUrl}/`);

  seedDevUser(ROOT);

  log('Building virtue-linux client');
  run(['cargo', 'build', '-p', 'virtue-linux'], { cwd: CLIENT_DIR });
  virtueBin = join(CLIENT_DIR, 'target/debug/virtue');

  log('Writing isolated client config');
  mkdirSync(join(xdgConfigHome, 'virtue'), { recursive: true });
  await Bun.write(
    join(xdgConfigHome, 'virtue/config.json'),
    JSON.stringify(
      {
        api_base_url: apiBaseUrl,
        capture_interval_seconds: CAPTURE_INTERVAL_SECONDS,
        batch_window_seconds: BATCH_WINDOW_SECONDS,
      },
      null,
      2,
    ),
  );

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

  await verifyDeviceHashBatch(API_DIR, DEVICE_NAME);
}

await runIntegrationTest(main, dumpLogs);
await cleanup();
process.exit(process.exitCode ?? 0);
