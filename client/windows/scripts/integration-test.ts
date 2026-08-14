#!/usr/bin/env bun
// Device -> api/hash-server integration smoke test (Windows).
//
// Boots the api worker locally against a fresh D1 database (the api's own
// D1-backed /hash routes stand in for the standalone Rust hash-server in
// local dev -- see api/src/lib/hash-server.ts and scripts/launch.sh), seeds
// the deterministic dev account, builds and runs a small
// `virtue-windows-ci-runner` binary that drives the real `virtue_windows`
// monitoring code in-process, then asserts that hashes and batches actually
// landed in the database.
//
// Unlike Linux (`virtue login` CLI) and macOS (a daemon binary + an
// IPC-socket login helper), the Windows client has no standalone daemon
// process or CLI at all -- `virtue_windows` is purely a cdylib the WinUI app
// loads via P/Invoke, and monitoring/login both happen as in-process calls
// against a background thread that same process spawns (see
// `RustInteropClient.cs`/`SessionViewModel.cs` for the app's own call
// sequence: Initialize -> StartMonitoring -> Login). `ci-runner.rs`
// reproduces that sequence directly against the `virtue-windows` library,
// then blocks for a fixed run window so the monitor's background thread can
// actually capture/hash/batch/upload before the process exits (which would
// otherwise kill that thread immediately), and exits on its own once that
// window elapses -- there's no separate daemon process to start, log in to,
// and kill.
//
// GitHub's windows-latest runners have a real interactive desktop session,
// so GDI screen capture (capture.rs) produces a genuine screenshot with no
// permission prompt and no virtual-display trick needed (unlike Linux's
// Xvfb or macOS's missing Screen Recording grant).
//
// Usage: bun client/windows/scripts/integration-test.ts
//
// Requires: bun, cargo, all on PATH. Windows only.

import { mkdtempSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { Subprocess } from 'bun';
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
} from '../../scripts/integration-test-lib.ts';

const SCRIPT_DIR = import.meta.dir;
const ROOT = join(SCRIPT_DIR, '../../..');
const CLIENT_DIR = join(ROOT, 'client');
const API_DIR = join(ROOT, 'api');

// capture_interval_seconds has a 15s floor enforced by client/core/src/config.rs.
const CAPTURE_INTERVAL_SECONDS = 15;
const BATCH_WINDOW_SECONDS = 15;
const RUN_DURATION_SECONDS = 60;

if (process.platform !== 'win32') {
  console.error('integration-test: this script only runs on Windows');
  process.exit(1);
}

requireCommands(['bun', 'cargo']);

const logDir = mkdtempSync(join(tmpdir(), 'virtue-windows-ci-log-'));
const apiLog = join(logDir, 'api.log');
const runnerLog = join(logDir, 'runner.log');

// Isolated PROGRAMDATA for the client under test only -- NOT set for the
// whole process's environment. `ClientPaths::discover()` resolves
// everything off PROGRAMDATA (see client/windows/src/config.rs), so
// overriding it just for the runner process below isolates
// %ProgramData%\Virtue the same way Linux isolates XDG_CONFIG_HOME/
// XDG_STATE_HOME and macOS isolates $HOME -- without touching a real local
// `virtue` install.
const tmpProgramData = mkdtempSync(join(tmpdir(), 'virtue-windows-ci-programdata-'));
const clientEnv: Record<string, string> = {
  ...(process.env as Record<string, string>),
  PROGRAMDATA: tmpProgramData,
};

let apiProc: Subprocess | undefined;
let runnerProc: Subprocess | undefined;

async function cleanup(): Promise<void> {
  await stopProcess(runnerProc);
  await stopProcess(apiProc);
  rmSync(tmpProgramData, { recursive: true, force: true });
  rmSync(logDir, { recursive: true, force: true });
}

async function dumpLogs(): Promise<void> {
  console.log('=== api log ===');
  if (existsSync(apiLog)) console.log(await Bun.file(apiLog).text());
  console.log('=== runner log ===');
  if (existsSync(runnerLog)) console.log(await Bun.file(runnerLog).text());
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

  log('Building virtue-windows-ci-runner');
  run(
    [
      'cargo',
      'build',
      '--target',
      'x86_64-pc-windows-msvc',
      '-p',
      'virtue-windows',
      '--bin',
      'virtue-windows-ci-runner',
    ],
    { cwd: CLIENT_DIR },
  );
  const runnerBin = join(
    CLIENT_DIR,
    'target/x86_64-pc-windows-msvc/debug/virtue-windows-ci-runner.exe',
  );

  log('Running the client (init/login/capture/batch)');
  runnerProc = spawnLogged(
    [
      runnerBin,
      '--api-base-url',
      apiBaseUrl,
      '--email',
      DEV_EMAIL,
      '--password',
      DEV_PASSWORD,
      '--device-name',
      DEVICE_NAME,
      '--capture-interval-seconds',
      String(CAPTURE_INTERVAL_SECONDS),
      '--batch-window-seconds',
      String(BATCH_WINDOW_SECONDS),
      '--run-duration-seconds',
      String(RUN_DURATION_SECONDS),
    ],
    { env: clientEnv, logPath: runnerLog },
  );

  // The runner blocks for RUN_DURATION_SECONDS itself (login, then a
  // capture/batch window) and exits on its own -- no separate sleep needed.
  const exitCode = await runnerProc.exited;
  if (exitCode !== 0) fail(`virtue-windows-ci-runner exited with code ${exitCode}`);

  await verifyDeviceHashBatch(API_DIR, DEVICE_NAME);
}

await runIntegrationTest(main, dumpLogs);
await cleanup();
process.exit(process.exitCode ?? 0);
