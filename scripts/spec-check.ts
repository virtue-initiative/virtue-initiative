#!/usr/bin/env bun
// Checks that every test in the repo cites a SPEC.md section ID, and shows
// which spec sections have no test.
//
//   bun scripts/spec-check.ts                    # CI check, exits 1 on problems
//   bun scripts/spec-check.ts --report           # markdown coverage report, never fails
//   bun scripts/spec-check.ts --write-allowlist  # rewrite scripts/spec-coverage-allow.txt
//   bun scripts/spec-check.ts --root <dir>       # run against another tree (for fixtures)
//
// Spec sections are headings like `## API-038 ...` in any SPEC.md. A test cites
// a section through an ID in its title, in a comment/attribute block above it,
// or on an enclosing describe/mod/class (see the per-language rules below).
// A test that guards no product behavior (a test-helper self-test) cites
// `NO-SPEC` in the same places instead.
// scripts/spec-coverage-allow.txt lists sections that have no test yet. It is a
// ratchet: an uncovered section must be listed, and a listed section that gains
// a test must be removed.
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'fs';
import { dirname, join, relative, resolve } from 'path';
import { fileURLToPath } from 'url';

type Lang = 'ts' | 'rust' | 'csharp' | 'kotlin' | 'swift';
type Section = { id: string; title: string; file: string; line: number };
type Ref = { id: string; line: number };
type Test = { file: string; line: number; lang: Lang; name: string; refs: Ref[] };
type Allow = { reason: string; line: number };

const argv = process.argv.slice(2);
const rootArg = argv.indexOf('--root');
const ROOT =
  rootArg >= 0 ? resolve(argv[rootArg + 1]) : join(dirname(fileURLToPath(import.meta.url)), '..');
const ALLOW_FILE = join(ROOT, 'scripts', 'spec-coverage-allow.txt');
const ALLOW_HEADER = `# Spec sections that have no test yet. One per line: \`ID  reason\`.
#
# This is a ratchet. \`bun scripts/spec-check.ts\` fails if an uncovered section is
# missing from this list, and also if a listed section is now cited by a test,
# so the list can only shrink. Delete the line when you add the test.
# \`bun scripts/spec-check.ts --write-allowlist\` adds new uncovered sections
# with the reason \`todo\`. Use a real reason where there is nothing to test,
# e.g. \`overview, nothing to test\`.
`;

// ---------- file discovery ----------

const SKIP_DIRS = new Set(['node_modules', 'target', 'build', 'dist', 'bin', 'obj', 'Pods']);

function walk(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith('.') || SKIP_DIRS.has(entry.name)) continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) walk(full, out);
    else if (entry.isFile()) out.push(relative(ROOT, full));
  }
  return out;
}

function langOf(file: string): Lang | undefined {
  const parts = file.split('/');
  const dirs = parts.slice(0, -1);
  if (/\.test\.tsx?$/.test(file)) return 'ts';
  if (file.endsWith('.rs') && ['client', 'hash-server'].includes(parts[0])) return 'rust';
  if (file.endsWith('.cs') && dirs.some((d) => d.endsWith('.Tests'))) return 'csharp';
  if (file.endsWith('.kt') && /(^|\/)src\/(test|androidTest)\//.test(file)) return 'kotlin';
  if (file.endsWith('.swift') && dirs.some((d) => d.endsWith('Tests'))) return 'swift';
}

const read = (file: string) => readFileSync(join(ROOT, file), 'utf8');

// ---------- spec parsing ----------

function parseSpecs(files: string[]) {
  const sections: Section[] = [];
  for (const file of files.filter((f) => f.split('/').pop() === 'SPEC.md')) {
    let fenced = false;
    read(file)
      .split('\n')
      .forEach((text, i) => {
        // A fence can open inside a list item (`- \`\`\`js`).
        if (/^\s*(?:[-*+]\s+|\d+\.\s+)?(```|~~~)/.test(text)) fenced = !fenced;
        const m = !fenced && /^#{1,6}\s+([A-Z]+-\d+)\b\s*(.*)$/.exec(text);
        if (m) sections.push({ id: m[1], title: m[2].replace(/`/g, '').trim(), file, line: i + 1 });
      });
  }
  return sections;
}

// ---------- ID extraction ----------

const NO_SPEC = 'NO-SPEC';

// Matches `PREFIX-123` for the prefixes found in the spec headings, and NO_SPEC.
function idRegex(sections: Section[]): RegExp {
  const prefixes = [...new Set(sections.map((s) => s.id.split('-')[0]))];
  return new RegExp(`\\b(?:(?:${prefixes.join('|') || '(?!)'})-\\d+|${NO_SPEC})\\b`, 'g');
}

let ID_RE: RegExp;

function refsIn(text: string, line: number): Ref[] {
  return [...text.matchAll(ID_RE)].map((m) => ({ id: m[0], line }));
}

function refsInLines(lines: string[], from: number, to: number): Ref[] {
  return lines.slice(from, to).flatMap((text, i) => refsIn(text, from + i + 1));
}

// ---------- shared helpers for indentation-based enclosure ----------

const indentOf = (text: string) => text.match(/^[ \t]*/)![0].replace(/\t/g, '    ').length;
const isBlank = (text: string) => text.trim() === '';
// Lines that cannot be the header of an enclosing scope.
const NOT_A_HEADER = /^\s*(\{|\}|\)|\]|\/\/|\/\*|\*|#\[|@|\[)/;

/**
 * Walks upward from `from`, returning every container whose indentation is
 * strictly smaller than the current threshold, innermost first. Any other
 * less-indented line is treated as an unrelated scope header and lowers the
 * threshold without matching.
 */
function enclosing<T>(lines: string[], from: number, container: (i: number) => T | undefined) {
  const found: T[] = [];
  let threshold = indentOf(lines[from]);
  for (let i = from - 1; i >= 0 && threshold > 0; i--) {
    if (isBlank(lines[i]) || indentOf(lines[i]) >= threshold) continue;
    const c = container(i);
    if (c) found.push(c);
    else if (NOT_A_HEADER.test(lines[i])) continue;
    threshold = indentOf(lines[i]);
  }
  return found;
}

// Start of the contiguous comment/attribute block directly above line `i`.
function blockStart(lines: string[], i: number, blockLine: RegExp): number {
  let s = i;
  while (s > 0 && blockLine.test(lines[s - 1])) s--;
  return s;
}

// ---------- TypeScript (vitest-style it/test/describe) ----------

const TS_COMMENT = /^\s*(\/\/|\/\*|\*)/;
const TS_ARG_MODIFIERS = new Set(['each', 'for', 'skipIf', 'runIf']); // followed by (args) or `table`
const TS_MODIFIERS = new Set(['skip', 'only', 'fails', 'todo', 'concurrent', 'sequential']);
const QUOTES = `"'\``;

function skipString(text: string, pos: number): number {
  for (let i = pos + 1; i < text.length; i++) {
    if (text[i] === '\\') i++;
    else if (text[i] === text[pos]) return i + 1;
  }
  return text.length;
}

// Index just past the bracket group starting at `pos`, or, with `toArgEnd`,
// the index of the next top-level comma or closing bracket.
function scan(text: string, pos: number, toArgEnd: boolean): number {
  let depth = 0;
  for (let i = pos; i < text.length; i++) {
    const c = text[i];
    if (QUOTES.includes(c)) i = skipString(text, i) - 1;
    else if (c === '/' && text[i + 1] === '/')
      i = text.includes('\n', i) ? text.indexOf('\n', i) : text.length; // skip comment
    else if ('([{'.includes(c)) depth++;
    else if (')]}'.includes(c)) {
      if (depth === 0) return i;
      if (--depth === 0 && !toArgEnd) return i + 1;
    } else if (c === ',' && depth === 0 && toArgEnd) return i;
  }
  return text.length;
}

// Reads `.mod`/`.each(...)` chains after it/test/describe, then returns the
// raw source of the title argument ('' when the title is not a literal) and its
// offset, or undefined if this is not a test call.
function readTsTitle(text: string, start: number): { title: string; at: number } | undefined {
  let pos = start;
  while (text[pos] === '.') {
    const mod = /^\.(\w+)/.exec(text.slice(pos, pos + 30))?.[1];
    if (!mod || !(TS_ARG_MODIFIERS.has(mod) || TS_MODIFIERS.has(mod))) return undefined;
    pos += mod.length + 1;
    if (!TS_ARG_MODIFIERS.has(mod)) continue;
    if (text[pos] === '`') pos = skipString(text, pos);
    else if (text[pos] === '(') pos = scan(text, pos, false);
    else return undefined;
  }
  if (text[pos] !== '(') return undefined;
  pos = text.slice(pos + 1).search(/\S/) + pos + 1;
  if (!QUOTES.includes(text[pos])) return { title: '', at: pos };
  return { title: text.slice(pos, scan(text, pos, true)), at: pos };
}

function parseTs(file: string): Test[] {
  const text = read(file);
  const lines = text.split('\n');
  const lineOf = (offset: number) => text.slice(0, offset).split('\n').length; // 1-based
  const describes = new Map<number, Ref[]>(); // keyed by 0-based line
  const tests: Test[] = [];

  const commentRefs = (line0: number) =>
    refsInLines(lines, blockStart(lines, line0, TS_COMMENT), line0);

  for (const m of text.matchAll(/^[ \t]*(it|test|describe)(?=[.(`])/gm)) {
    const call = readTsTitle(text, m.index + m[0].length);
    if (!call) continue;
    const line0 = lineOf(m.index) - 1;
    const refs = [
      ...commentRefs(line0),
      ...[...call.title.matchAll(ID_RE)].map((r) => ({
        id: r[0],
        line: lineOf(call.at + r.index),
      })),
    ];
    if (m[1] === 'describe') {
      describes.set(line0, refs);
      continue;
    }
    const outer = enclosing(lines, line0, (i) => describes.get(i));
    tests.push({
      file,
      line: line0 + 1,
      lang: 'ts',
      name: call.title.slice(1, -1) || '(untitled)',
      refs: [...refs, ...outer.flat()],
    });
  }
  return tests;
}

// ---------- Rust / C# / Kotlin / Swift ----------

type Other = Exclude<Lang, 'ts'>;

// Comment and attribute lines that belong to the declaration below them.
const BLOCK_LINE: Record<Other, RegExp> = {
  rust: /^\s*(\/\/|\/\*|\*|#\[)/,
  csharp: /^\s*(\/\/|\/\*|\*|\[)/,
  kotlin: /^\s*(\/\/|\/\*|\*|@)/,
  swift: /^\s*(\/\/|\/\*|\*|@)/,
};
const TEST_MARKER: Record<Other, RegExp> = {
  rust: /^\s*#\[\s*(?:[\w:]*::)?(?:test|rstest)\b[^\]]*\]\s*$/,
  csharp: /^\s*\[\s*(?:Fact|Theory|Test|TestMethod)\b/,
  kotlin: /^\s*@Test\b/,
  swift: /^\s*@Test\b/,
};
const CONTAINER: Record<Other, RegExp> = {
  rust: /^\s*(pub(\([^)]*\))?\s+)?(mod|struct|impl|enum|trait)\b/,
  csharp: /^\s*(\w+\s+)*(class|struct|record|interface)\s+\w+/,
  kotlin: /^\s*(\w+\s+)*(class|object|interface)\b/,
  swift: /^\s*(@\w+(\([^)]*\))?\s+)*(\w+\s+)*(class|struct|extension|enum|actor)\b/,
};
const SWIFT_XCTEST = /^\s*(\w+\s+)*func\s+test\w*\s*[(<]/;
const INLINE_DECL = /\b(fun|func)\s/; // `@Test fun x()` on one line

function declName(text: string): string {
  return /`([^`]+)`/.exec(text)?.[1] ?? /(\w+)\s*(<[^>]*>)?\s*\(/.exec(text)?.[1] ?? text.trim();
}

// Rust: `//!` lines at the top of the file apply to every test in it.
function rustFileDocRefs(lines: string[]): Ref[] {
  const refs: Ref[] = [];
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].startsWith('//!')) refs.push(...refsIn(lines[i], i + 1));
    else if (!isBlank(lines[i]) && !/^\s*(\/\/|#!\[)/.test(lines[i])) break;
  }
  return refs;
}

function parseOther(file: string, lang: Other): Test[] {
  const lines = read(file).split('\n');
  const blockLine = BLOCK_LINE[lang];
  const fileRefs = lang === 'rust' ? rustFileDocRefs(lines) : [];
  const tests: Test[] = [];

  for (let i = 0; i < lines.length; i++) {
    const start = blockStart(lines, i, blockLine);
    const isBlock = blockLine.test(lines[i]);
    const inline = isBlock && TEST_MARKER[lang].test(lines[i]) && INLINE_DECL.test(lines[i]);
    const afterInline = i > 0 && INLINE_DECL.test(lines[i - 1]); // `@Test fun x()` owns the block
    const marked =
      !isBlock && !afterInline && lines.slice(start, i).some((l) => TEST_MARKER[lang].test(l));
    const xctest = lang === 'swift' && !isBlock && SWIFT_XCTEST.test(lines[i]);
    if (!inline && !marked && !xctest) continue;

    const refs = [...fileRefs, ...refsInLines(lines, start, inline ? i + 1 : i)];
    const outer = enclosing(lines, i, (j) => (CONTAINER[lang].test(lines[j]) ? j : undefined));
    for (const j of outer) refs.push(...refsInLines(lines, blockStart(lines, j, blockLine), j));
    tests.push({ file, line: i + 1, lang, name: declName(lines[i]), refs });
  }
  return tests;
}

// ---------- allowlist ----------

function parseAllowlist(problems: string[]): Map<string, Allow> {
  const allow = new Map<string, Allow>();
  if (!existsSync(ALLOW_FILE)) return allow;
  const file = relative(ROOT, ALLOW_FILE);
  read(file)
    .split('\n')
    .forEach((text, i) => {
      if (isBlank(text) || text.trim().startsWith('#')) return;
      const m = /^([A-Z]+-\d+)\s*(.*)$/.exec(text.trim());
      if (!m) problems.push(`${file}:${i + 1}: expected "ID  reason", got "${text.trim()}"`);
      else allow.set(m[1], { reason: m[2] || 'todo', line: i + 1 });
    });
  return allow;
}

function writeAllowlist(uncovered: Section[], allow: Map<string, Allow>) {
  const width = Math.max(0, ...uncovered.map((s) => s.id.length));
  const body: string[] = [];
  let file = '';
  for (const s of uncovered) {
    if (s.file !== file) body.push('', `# ${(file = s.file)}`);
    body.push(`${s.id.padEnd(width)}  ${allow.get(s.id)?.reason ?? 'todo'}`);
  }
  mkdirSync(dirname(ALLOW_FILE), { recursive: true });
  writeFileSync(ALLOW_FILE, `${ALLOW_HEADER}${body.join('\n')}\n`);
}

// ---------- main ----------

const files = walk(ROOT);
const sections = parseSpecs(files);
ID_RE = idRegex(sections);
const sectionIds = new Set(sections.map((s) => s.id));
const testFiles = files.flatMap((f) => {
  const lang = langOf(f);
  return lang ? [{ file: f, lang }] : [];
});
const tests = testFiles.flatMap(({ file, lang }) =>
  lang === 'ts' ? parseTs(file) : parseOther(file, lang),
);

const citing = new Map<string, number>();
for (const t of tests) {
  for (const id of new Set(t.refs.map((r) => r.id))) citing.set(id, (citing.get(id) ?? 0) + 1);
}
const isTagged = (t: Test) => t.refs.length > 0;
const isExempt = (t: Test) => t.refs.some((r) => r.id === NO_SPEC);
const uncovered = sections.filter((s) => !citing.has(s.id));
const loc = (s: Section) => `${s.file}:${s.line}`;

function knownBugs() {
  return testFiles.flatMap(({ file }) =>
    read(file)
      .split('\n')
      .flatMap((text, i) => {
        const m = /KNOWN-BUG:\s*(.*)$/.exec(text);
        // Drop what closes the string or comment: `"]`, `");`, `*/`.
        const note = m?.[1].replace(/["'][\]);,\s]*$|\*\/\s*$/, '').trim();
        return m ? [`${file}:${i + 1} ${note}`] : [];
      }),
  );
}

function table(head: string[], rows: (string | number)[][]): string[] {
  const row = (cells: (string | number)[]) => `| ${cells.join(' | ')} |`;
  return [row(head), row(head.map(() => '---')), ...rows.map(row), ''];
}

function report() {
  const out = ['# Spec coverage', '', '## Tests by language', ''];
  const langs: Lang[] = ['ts', 'rust', 'csharp', 'kotlin', 'swift'];
  out.push(
    ...table(
      ['Language', 'Tests', 'Tagged', 'Untagged'],
      langs.map((lang) => {
        const ts = tests.filter((t) => t.lang === lang);
        const tagged = ts.filter(isTagged).length;
        return [lang, ts.length, tagged, ts.length - tagged];
      }),
    ),
  );

  for (const file of new Set(sections.map((s) => s.file))) {
    const rows = sections.filter((s) => s.file === file);
    out.push(`## ${file}`, '');
    out.push(
      ...table(
        ['ID', 'Title', 'Tests'],
        rows.map((s) => [s.id, s.title.replace(/\|/g, '\\|'), citing.get(s.id) ?? 0]),
      ),
    );
  }

  const allow = parseAllowlist([]);
  out.push(`## Uncovered sections (${uncovered.length})`, '');
  for (const s of uncovered) {
    const reason = allow.has(s.id) ? ` [allowed: ${allow.get(s.id)!.reason}]` : '';
    out.push(`- ${s.id} ${s.title} (${loc(s)})${reason}`);
  }
  const bugs = knownBugs();
  out.push(
    '',
    '## KNOWN-BUG markers',
    '',
    ...(bugs.length ? bugs.map((b) => `- ${b}`) : ['None.']),
  );
  const exempt = tests.filter(isExempt);
  out.push('', `## ${NO_SPEC} tests (${exempt.length})`, '');
  out.push(...(exempt.length ? exempt.map((t) => `- ${t.file}:${t.line} ${t.name}`) : ['None.']));

  const dirs = new Map<string, { total: number; untagged: number }>();
  for (const t of tests) {
    const d = dirs.get(dirname(t.file)) ?? { total: 0, untagged: 0 };
    d.total++;
    if (!isTagged(t)) d.untagged++;
    dirs.set(dirname(t.file), d);
  }
  const untaggedDirs = [...dirs].filter(([, d]) => d.untagged);
  untaggedDirs.sort((a, b) => b[1].untagged - a[1].untagged);
  out.push('', '## Untagged tests by directory', '');
  out.push(
    ...table(
      ['Directory', 'Untagged', 'Tests'],
      untaggedDirs.map(([dir, d]) => [dir, d.untagged, d.total]),
    ),
  );
  console.log(out.join('\n'));
}

// Problems are grouped under a heading; empty groups are dropped.
function check(): string[] {
  const problems: string[] = [];
  const group = (heading: string, items: string[]) => {
    if (items.length) problems.push(heading, ...items.map((i) => `  ${i}`), '');
  };
  const specFiles = new Set(sections.map((s) => s.file));

  group(
    'Tests that cite no spec section ID (add one, or NO-SPEC, to the title, a comment above, or an enclosing describe/mod/class):',
    tests.filter((t) => !isTagged(t)).map((t) => `${t.file}:${t.line}  ${t.name}`),
  );

  // Unknown IDs: whole files for test-only files and SPEC.md. Rust files outside
  // tests/ also hold source code, so only the IDs attached to their tests count.
  const unknown = new Set<string>();
  const checkRefs = (file: string, refs: Ref[]) =>
    refs
      .filter((r) => r.id !== NO_SPEC && !sectionIds.has(r.id))
      .forEach((r) => unknown.add(`${file}:${r.line}  ${r.id}`));
  for (const { file, lang } of testFiles) {
    const wholeFile = lang !== 'rust' || file.split('/').includes('tests');
    checkRefs(
      file,
      wholeFile
        ? refsInLines(read(file).split('\n'), 0, Infinity)
        : tests.filter((t) => t.file === file).flatMap((t) => t.refs),
    );
  }
  for (const file of specFiles) checkRefs(file, refsInLines(read(file).split('\n'), 0, Infinity));
  group('References to spec IDs that do not exist in any SPEC.md:', [...unknown]);

  const first = new Map<string, Section>();
  group(
    'Duplicate spec section IDs:',
    sections.flatMap((s) => {
      if (!first.has(s.id)) first.set(s.id, s);
      return first.get(s.id) === s
        ? []
        : [`${loc(s)}  ${s.id} is already defined at ${loc(first.get(s.id)!)}`];
    }),
  );

  const allow = parseAllowlist(problems);
  const allowFile = relative(ROOT, ALLOW_FILE);
  group(
    `Spec sections with no test (write a test citing the ID, or run --write-allowlist to add it to ${allowFile}):`,
    uncovered.filter((s) => !allow.has(s.id)).map((s) => `${loc(s)}  ${s.id} ${s.title}`),
  );
  group(
    `Sections listed in ${allowFile} that no longer need to be (remove the line):`,
    [...allow]
      .filter(([id]) => !sectionIds.has(id) || citing.has(id))
      .map(([id, a]) => {
        const why = sectionIds.has(id) ? 'is now cited by a test' : 'no longer exists';
        return `${allowFile}:${a.line}  ${id} ${why}`;
      }),
  );
  return problems;
}

if (argv.includes('--report')) {
  report();
} else if (argv.includes('--write-allowlist')) {
  writeAllowlist(uncovered, parseAllowlist([]));
  console.log(`Wrote ${uncovered.length} uncovered section(s) to ${relative(ROOT, ALLOW_FILE)}.`);
} else {
  const problems = check();
  if (problems.length) {
    console.error(problems.join('\n'));
    console.error('spec-check failed.');
    process.exit(1);
  }
  console.log(`spec-check ok: ${tests.length} tests, ${sections.length} spec sections.`);
}
