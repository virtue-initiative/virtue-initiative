#!/usr/bin/env bun
// Device -> api/hash-server integration smoke test (macOS).
//
// Boots the api worker locally alongside a real standalone hash-server
// process (see hash-server/SPEC.md and scripts/launch.sh, which starts it
// the same way for local dev), seeds the deterministic dev account, builds
// and runs the real virtue-mac daemon binary directly (no launchd, no
// packaged .app), logs it in over its IPC socket, waits for a real
// screenshot/hash/batch cycle, then asserts that a device/batch landed in D1
// and that the hash server actually ingested a hash for it.
//
// Screen Recording permission: CI runners don't have it granted, and macOS's
// TCC framework has no scriptable/headless way to grant it (unlike Linux's
// Xvfb, which just gives the daemon a permission-free virtual display).
// Rather than testing the CaptureFailed alert fallback instead of a real
// capture, the daemon here is built with the mock-capture feature
// (client/mac/src/capture.rs), which swaps in a fixed embedded PNG in place
// of shelling out to `screencapture`. That's compiled in only when this
// script explicitly requests it -- never by build-app.sh/build-dmg.sh -- so
// it can't end up in a shipped build, and it still exercises the real
// capture -> classify -> upload -> hash -> batch pipeline end to end -- it
// just doesn't cover the Screen Recording permission-gating logic itself
// (CGPreflightScreenCaptureAccess / CaptureFailed), which stays untested by
// this job.
//
// Usage: bun client/mac/scripts/integration-test.ts
//
// Requires: bun, cargo, curl, all on PATH. macOS only.

import { mkdirSync, mkdtempSync, existsSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import type { Subprocess } from 'bun';
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
// One capture interval for the (mocked) screenshot to fire, plus one batch
// window for it to flush, plus margin for CI scheduling jitter.
const RUN_DURATION_SECONDS = 45;

if (process.platform !== 'darwin') {
  console.error('integration-test: this script only runs on macOS');
  process.exit(1);
}

requireCommands(['bun', 'cargo', 'curl']);

const logDir = mkdtempSync('/tmp/virtue-mac-ci-log-');
const apiLog = join(logDir, 'api.log');
const hashLog = join(logDir, 'hash-server.log');
const daemonLog = join(logDir, 'daemon.log');

// Isolated HOME for the client under test only -- NOT exported globally. On
// macOS `dirs::config_dir()`/`data_dir()`/`home_dir()` all resolve off $HOME,
// so overriding it isolates ~/Library/Application Support/virtue and
// ~/Library/LaunchAgents the same way Linux isolates XDG_CONFIG_HOME/
// XDG_STATE_HOME -- without touching a real local `virtue` install.
//
// Deliberately rooted at /tmp rather than the system temp dir (mkdtemp with
// no explicit prefix resolves under $TMPDIR, e.g.
// /var/folders/xx/xxxxxxxxxxxxxxxxxxxxxxxxxxxx/T on macOS): daemon.sock's
// full path must fit in sockaddr_un.sun_path, capped at 104 bytes on macOS,
// and "$TMPDIR/.../Library/Application Support/virtue/state/daemon.sock"
// alone already exceeds that -- IpcBridge::bind() then fails, and since IPC
// is treated as optional (the daemon still runs without a controller
// connection), it fails silently with no error in the daemon log, just a
// socket that never appears.
const tmpHome = mkdtempSync('/tmp/virtue-mac-ci-home-');
const clientAppSupport = join(tmpHome, 'Library/Application Support/virtue');
const clientEnv: Record<string, string> = {
  ...(process.env as Record<string, string>),
  HOME: tmpHome,
};

let apiProc: Subprocess | undefined;
let hashProc: Subprocess | undefined;
let daemonProc: Subprocess | undefined;

async function cleanup(): Promise<void> {
  await stopProcess(daemonProc);
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

  log('Building virtue-mac client (mock-capture)');
  run(['cargo', 'build', '-p', 'virtue-mac', '--features', 'mock-capture'], { cwd: CLIENT_DIR });
  const virtueBin = join(CLIENT_DIR, 'target/debug/virtue-mac');
  const ciLoginBin = join(CLIENT_DIR, 'target/debug/virtue-mac-ci-login');

  log('Writing isolated client config');
  mkdirSync(clientAppSupport, { recursive: true });
  await Bun.write(
    join(clientAppSupport, 'config.json'),
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

  log('Starting the daemon');
  daemonProc = spawnLogged([virtueBin, 'daemon'], { env: clientEnv, logPath: daemonLog });

  log('Waiting for the daemon IPC socket');
  const daemonSock = join(clientAppSupport, 'state/daemon.sock');
  const daemonReady = await waitUntil(30, () => existsSync(daemonSock));
  if (!daemonReady) fail('daemon did not create its IPC socket in time');

  log('Logging in');
  run([
    ciLoginBin,
    '--socket',
    daemonSock,
    '--email',
    DEV_EMAIL,
    '--password',
    DEV_PASSWORD,
    '--device-name',
    DEVICE_NAME,
  ]);

  log(`Waiting ${RUN_DURATION_SECONDS}s for capture/batch/hash activity`);
  await new Promise((resolve) => setTimeout(resolve, RUN_DURATION_SECONDS * 1000));

  const jwtPrivateKeyPem = readDevVar(API_DIR, 'JWT_PRIVATE_KEY');
  await verifyDeviceHashBatch(API_DIR, HASH_SERVER_DIR, hashBaseUrl, jwtPrivateKeyPem, DEVICE_NAME);
}

await runIntegrationTest(main, dumpLogs);
await cleanup();
process.exit(process.exitCode ?? 0);
