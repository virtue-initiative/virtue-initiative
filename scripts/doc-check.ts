#!/usr/bin/env bun
// Checks that the repo's markdown docs only point at files that exist.
//
//   bun scripts/doc-check.ts            # exits 1 if any doc names a missing file
//   bun scripts/doc-check.ts --root <dir>
//
// Two kinds of reference are checked, outside fenced code blocks:
//
// - Relative markdown links, `[text](path)`, resolved against the doc's directory.
// - Inline code that looks like a repo path (`web/src/index.tsx`, `scripts/`): it
//   must contain a `/` and end in a file extension or a `/`. It may be written
//   relative to the doc's directory or to any directory above it, with or
//   without a `src/` in between, so `src/lib/db.ts` in `api/CLAUDE.md` and
//   `module/upload.rs` in `client/core/architecture.md` both resolve.
//
// Absolute paths, `~` paths, host names, placeholders (`<version>`, `*`, `$VAR`,
// `%VAR%`), build output (`target/`, `dist/`, ...) and gitignored local files
// (`api/.dev.vars`) are skipped. To skip one line on purpose,
// put `<!-- doc-check: ignore -->` on it or on the line above (blank lines between are fine).
import { execFileSync } from 'child_process';
import { existsSync, readFileSync } from 'fs';
import { dirname, join, posix, resolve } from 'path';
import { fileURLToPath } from 'url';

const argv = process.argv.slice(2);
const rootArg = argv.indexOf('--root');
const ROOT =
  rootArg >= 0 ? resolve(argv[rootArg + 1]) : join(dirname(fileURLToPath(import.meta.url)), '..');

// User-facing site content links to site routes, and vendored skill references
// describe someone else's project.
const SKIP_FILES = [/^landing\/src\/(content|legal)\//, /^\.claude\/skills\/playwright-cli\//];
const OUTPUT_DIRS = new Set([
  'target',
  'target-docker',
  'dist',
  'build',
  'node_modules',
  '.derived-data',
  '.playwright-cli',
  '.newsletter',
  'remote-logs',
]);
const IGNORE = '<!-- doc-check: ignore -->';

const files = execFileSync('git', ['ls-files', '*.md'], { cwd: ROOT, encoding: 'utf8' })
  .split('\n')
  .filter((f) => f && !SKIP_FILES.some((re) => re.test(f)));

function exists(rel: string) {
  return existsSync(join(ROOT, rel));
}

function resolveLink(doc: string, target: string): boolean {
  const path = decodeURIComponent(target.split('#')[0]);
  if (!path) return true;
  const rel = posix.join(posix.dirname(doc), path);
  return exists(rel) || ignored(rel);
}

function looksLikePath(code: string): boolean {
  if (!code.includes('/') || /[\s<>*{}$%~:@`'"=|]/.test(code)) return false;
  if (code.startsWith('/') || code.startsWith('-') || code.startsWith('http')) return false;
  if (!/^[\w./-]+$/.test(code)) return false;
  if (/^[\w-]+(\.[\w-]+)*\.(org|com|net|io|dev|app)\//.test(code)) return false;
  const segments = code.split('/').filter(Boolean);
  if (segments.some((s) => OUTPUT_DIRS.has(s) || s.startsWith('.derived-data'))) return false;
  return code.endsWith('/') || /\.[A-Za-z][\w]*$/.test(segments[segments.length - 1]);
}

// A local file such as `api/.dev.vars` or `web/.env.local` exists only after
// setup, so a path git ignores counts as present.
function ignored(rel: string): boolean {
  if (rel.startsWith('..')) return false;
  try {
    execFileSync('git', ['check-ignore', '-q', '--no-index', rel], { cwd: ROOT, stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

function resolveCode(doc: string, code: string): boolean {
  const path = code.replace(/^\.\//, '');
  const candidates: string[] = [];
  let dir = posix.dirname(doc);
  for (;;) {
    const base = dir === '.' ? '' : dir;
    candidates.push(posix.join(base, path), posix.join(base, 'src', path));
    if (dir === '.') break;
    dir = posix.dirname(dir);
  }
  return candidates.some(exists) || candidates.some(ignored);
}

// The nearest non-blank line above, since Prettier puts a blank line after an HTML comment.
function previousLine(lines: string[], i: number): string {
  for (let j = i - 1; j >= 0; j--) if (lines[j].trim()) return lines[j];
  return '';
}

const problems: string[] = [];
for (const doc of files) {
  const lines = readFileSync(join(ROOT, doc), 'utf8').split('\n');
  let fence: string | null = null;
  lines.forEach((line, i) => {
    const fenceMatch = line.match(/^\s*(```|~~~)/);
    if (fenceMatch) {
      fence = fence === null ? fenceMatch[1] : fence === fenceMatch[1] ? null : fence;
      return;
    }
    if (fence !== null || line.includes(IGNORE) || previousLine(lines, i).includes(IGNORE)) return;

    for (const m of line.matchAll(/\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g)) {
      const target = m[1];
      if (/^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith('#')) continue;
      if (target.startsWith('/')) continue;
      if (!resolveLink(doc, target)) problems.push(`${doc}:${i + 1}: broken link ${target}`);
    }
    for (const m of line.matchAll(/`([^`\n]+)`/g)) {
      const code = m[1].replace(/:\d+(-\d+)?$/, '');
      if (looksLikePath(code) && !resolveCode(doc, code)) {
        problems.push(`${doc}:${i + 1}: no such file ${code}`);
      }
    }
  });
}

if (problems.length) {
  console.error(problems.join('\n'));
  console.error(
    `\ndoc-check: ${problems.length} reference(s) to missing files. Fix the path, or add ${IGNORE} if the file is not meant to exist in the repo.`,
  );
  process.exit(1);
}
console.log(`doc-check ok: ${files.length} docs.`);
