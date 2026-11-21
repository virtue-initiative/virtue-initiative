#!/usr/bin/env bun
// Runs the CI checks for every area a branch touches, so you don't have to work out
// which ones apply. The commands match docs/ci-checks.md (minus release packaging).
//
//   bun scripts/check-changed.ts                # areas changed since staging, plus uncommitted work
//   bun scripts/check-changed.ts --base <ref>   # compare against another branch or commit
//   bun scripts/check-changed.ts --list         # print the commands without running them
//   bun scripts/check-changed.ts api web        # run these areas, whatever changed
//
// Areas run in parallel; each area's commands run in order. Passing commands print one
// line; a failing one prints the end of its output. Areas this OS can't build (macOS,
// Windows, iOS, Android without an SDK) are listed with where to run them instead.
import { execFileSync } from 'child_process';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const argv = process.argv.slice(2);
const flag = (name: string) => argv.includes(name);
const option = (name: string) => (argv.includes(name) ? argv[argv.indexOf(name) + 1] : undefined);

// `docs`: also run when the area's only changes are markdown (Prettier covers it).
type Step = { cwd: string; cmd: string; env?: Record<string, string>; docs?: boolean };
type Area = {
  name: string;
  matches: (file: string) => boolean;
  steps: Step[];
  // Why it can't run here, and what to run instead; undefined when it can.
  unavailable?: () => string | undefined;
};

const bunChecks = (cwd: string, scripts: string[], env?: Record<string, string>): Step[] =>
  scripts.map((s) => ({ cwd, cmd: `bun run ${s}`, env, docs: s === 'format:check' }));
// Markdown is prose, except the landing site's, which it builds into pages.
const isDoc = (f: string) => f.endsWith('.md') && !f.startsWith('landing/src/');
const under =
  (...dirs: string[]) =>
  (f: string) =>
    dirs.some((d) => f.startsWith(d));
const has = (cmd: string) => {
  try {
    execFileSync('sh', ['-c', `command -v ${cmd}`], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
};
const onlyOn = (os: NodeJS.Platform, label: string, elsewhere: string) => () =>
  process.platform === os ? undefined : `needs ${label}; ${elsewhere}`;

const coreRust = (cargo: string): Step[] => [
  { cwd: 'client', cmd: `${cargo} clippy -p virtue-core --all-targets -- -D warnings` },
  { cwd: 'client', cmd: `${cargo} test -p virtue-core` },
  { cwd: 'client', cmd: `${cargo} test -p virtue-core --features testing --tests` },
];
const webTs = ['typecheck', 'test', 'format:check', 'build'];
const landingEnv = { GITHUB_TOKEN: process.env.GITHUB_TOKEN ?? 'stub' };

const AREAS: Area[] = [
  {
    name: 'api',
    matches: under('api/', 'shared-web/'),
    steps: bunChecks('api', ['typecheck', 'test', 'format:check']),
  },
  {
    name: 'api-donate',
    matches: under('api-donate/'),
    steps: bunChecks('api-donate', ['typecheck', 'test', 'format:check']),
  },
  {
    name: 'web',
    matches: (f) => under('web/', 'shared-web/', 'batch-format/')(f) || f === 'theme.json',
    steps: bunChecks('web', webTs),
  },
  {
    name: 'landing',
    matches: (f) => under('landing/', 'shared-web/')(f) || f === 'theme.json',
    steps: bunChecks('landing', ['typecheck', 'format:check', 'build'], landingEnv),
  },
  {
    name: 'shared-web',
    matches: under('shared-web/'),
    steps: bunChecks('shared-web', ['format:check']),
  },
  {
    name: 'hash-server',
    matches: under('hash-server/', 'batch-format/'),
    steps: [
      { cwd: 'hash-server', cmd: 'cargo fmt --all -- --check' },
      { cwd: 'hash-server', cmd: 'cargo clippy --all-targets -- -D warnings' },
      { cwd: 'hash-server', cmd: 'cargo test' },
    ],
  },
  {
    // Linux CI runs for any Rust change in client/, and it is the baseline for core changes.
    name: 'client-linux',
    matches: (f) =>
      under('client/linux/', 'client/core/', 'batch-format/')(f) ||
      (f.startsWith('client/') && f.endsWith('.rs')),
    steps: [
      { cwd: 'client', cmd: 'cargo fmt --all -- --check' },
      ...coreRust('cargo'),
      { cwd: 'client', cmd: 'cargo clippy -p virtue-linux --all-targets -- -D warnings' },
      { cwd: 'client', cmd: 'cargo test -p virtue-linux' },
    ],
    unavailable: onlyOn('linux', 'Linux', 'CI runs it (client-linux.yml)'),
  },
  {
    name: 'client-mac',
    matches: under('client/mac/', 'client/shared-swift/', 'client/core/'),
    steps: [
      ...coreRust('cargo'),
      { cwd: 'client', cmd: 'cargo clippy -p virtue-mac --all-targets -- -D warnings' },
      { cwd: 'client/shared-swift', cmd: 'swift test' },
    ],
    unavailable: onlyOn('darwin', 'macOS', 'from Linux, `just mac-remote` builds it on the Mac'),
  },
  {
    name: 'client-windows',
    matches: under('client/windows/', 'client/core/'),
    steps: [
      ...coreRust('cargo'),
      { cwd: 'client', cmd: 'cargo clippy -p virtue-windows --all-targets -- -D warnings' },
      { cwd: 'client', cmd: 'cargo test -p virtue-windows' },
    ],
    unavailable: onlyOn(
      'win32',
      'Windows',
      'from Linux, `client/windows/scripts/remote-windows-build.sh --mode smoke` runs it in the VM',
    ),
  },
  {
    name: 'client-ios',
    matches: under('client/ios/', 'client/shared-swift/'),
    steps: [
      {
        cwd: 'client/ios',
        cmd: './scripts/build-ios.sh --destination "generic/platform=iOS Simulator" --derived-data .derived-data-ci-ios',
      },
    ],
    unavailable: onlyOn('darwin', 'macOS', 'from Linux, `just ios-remote` builds it on the Mac'),
  },
  {
    name: 'client-android',
    matches: under('client/android/'),
    steps: [
      {
        cwd: 'client/android',
        cmd: './gradlew --no-daemon :app:testDebugUnitTest :app:lintDebug :app:assembleDebug',
      },
    ],
    unavailable: () =>
      process.env.ANDROID_HOME || process.env.ANDROID_SDK_ROOT
        ? undefined
        : 'needs the Android SDK (ANDROID_HOME); see client/android/README.md',
  },
  {
    name: 'swift-format',
    matches: (f) => f.endsWith('.swift'),
    steps: [{ cwd: '.', cmd: './scripts/swift-format.sh --check' }],
    unavailable: () => (has('swift') ? undefined : 'needs a Swift toolchain; CI runs it'),
  },
  {
    // Fast, and any change can break a doc or a spec citation.
    name: 'specs-and-docs',
    matches: () => true,
    steps: [
      { cwd: '.', cmd: 'bun scripts/spec-check.ts' },
      { cwd: '.', cmd: 'bun scripts/doc-check.ts' },
    ],
  },
];

const show = (s: Step) => (s.cwd === '.' ? s.cmd : `(cd ${s.cwd} && ${s.cmd})`);

function git(...args: string[]): string[] {
  return execFileSync('git', args, { cwd: ROOT, encoding: 'utf8' }).split('\n').filter(Boolean);
}

function changedFiles(): string[] {
  const base =
    option('--base') ??
    (git('rev-parse', '--verify', '-q', 'origin/staging').length ? 'origin/staging' : 'staging');
  const [mergeBase] = git('merge-base', 'HEAD', base);
  return [
    ...new Set([
      ...git('diff', '--name-only', mergeBase),
      ...git('ls-files', '--others', '--exclude-standard'),
    ]),
  ];
}

const named = argv.filter((a, i) => !a.startsWith('--') && argv[i - 1] !== '--base');
const unknown = named.filter((n) => !AREAS.some((a) => a.name === n));
if (unknown.length) {
  console.error(
    `Unknown area(s): ${unknown.join(', ')}. Areas: ${AREAS.map((a) => a.name).join(', ')}`,
  );
  process.exit(2);
}
const files = named.length ? [] : changedFiles();
// An area whose only changes are docs runs just its `docs` steps.
const areas = named.length
  ? AREAS.filter((a) => named.includes(a.name))
  : AREAS.flatMap((a) => {
      const mine = files.filter(a.matches);
      if (a.name === 'specs-and-docs' || mine.some((f) => !isDoc(f))) return mine.length ? [a] : [];
      const steps = a.steps.filter((s) => s.docs);
      return mine.length && steps.length ? [{ ...a, steps }] : [];
    });
const runnable = areas.filter((a) => !a.unavailable?.());
const skipped = areas.filter((a) => a.unavailable?.());

if (!named.length)
  console.log(`${files.length} changed file(s); areas: ${areas.map((a) => a.name).join(', ')}`);
for (const a of skipped) console.log(`skip ${a.name}: ${a.unavailable!()}`);

if (flag('--list')) {
  for (const a of runnable) {
    console.log(`\n${a.name}`);
    for (const s of a.steps) console.log(`  ${show(s)}`);
  }
  process.exit(0);
}

// Runs every step even after a failure, so one run shows all of an area's problems.
async function runArea(area: Area): Promise<boolean> {
  let passed = true;
  for (const step of area.steps) {
    const started = performance.now();
    const proc = Bun.spawn(['sh', '-c', `${step.cmd} 2>&1`], {
      cwd: join(ROOT, step.cwd),
      env: { ...process.env, ...step.env, CI: '1', FORCE_COLOR: '0', CARGO_TERM_COLOR: 'never' },
      stdout: 'pipe',
    });
    const output = await new Response(proc.stdout).text();
    const ok = (await proc.exited) === 0;
    const secs = ((performance.now() - started) / 1000).toFixed(1);
    const line = `${ok ? 'ok  ' : 'FAIL'} ${area.name}: ${show(step)}  ${secs}s`;
    if (ok) {
      console.log(line);
      continue;
    }
    const tail = output.trimEnd().split('\n').slice(-80).join('\n');
    console.log(`${line}\n${tail}\n`);
    passed = false;
  }
  return passed;
}

const results = await Promise.all(runnable.map(runArea));
const failed = runnable.filter((_, i) => !results[i]).map((a) => a.name);
console.log(failed.length ? `\nFailed: ${failed.join(', ')}` : `\nAll checks passed.`);
if (skipped.length) console.log(`Not run here: ${skipped.map((a) => a.name).join(', ')}`);
process.exit(failed.length ? 1 : 0);
