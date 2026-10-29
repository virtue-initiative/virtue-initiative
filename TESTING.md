# Testing

The specs say what the software must do, and the tests prove it. A test that proves
a guarantee should still pass after the code behind it is rewritten, so tests check
behavior at a component's public interface, not its internals.

## Specs and tests

Every component with behavior worth guaranteeing has a `SPEC.md` (see the table in
`CLAUDE.md`). Each section has a stable ID such as `API-038` or `WEB-012`.

- Every test MUST cite at least one spec ID. `just spec-check` enforces this in CI.
- Cite the narrowest section that is true. Tag a whole `describe` block or Rust module
  only when every test in it proves the same section.
- An edge case worth guaranteeing goes in the spec as a sentence first, then as a test.
  If no sentence in the spec explains why a test exists, add one or delete the test.
- A test that guards no product behavior, such as a self-test of a test helper or fake,
  cites `NO-SPEC` instead, with a comment saying why. Use it rarely: if users or another
  component would notice the behavior, it belongs in a spec. `just spec-coverage` lists
  every `NO-SPEC` test.

Where the ID goes:

| Language            | Put the ID in                                                                                                    |
| ------------------- | ---------------------------------------------------------------------------------------------------------------- |
| TypeScript (Vitest) | The `it`/`test` title or an enclosing `describe` title, as a suffix: `describe('POST /d/batch (API-038)', ...)`  |
| Rust                | A comment directly above `#[test]`, or above the enclosing `mod`, or the file's `//!` header: `// CORE-002: ...` |
| C#, Kotlin, Swift   | A comment directly above the test method or its enclosing class                                                  |

## Where tests attach

Test through the interface a user or another component uses. Anything behind it is
free to change.

| Component             | Test through                                                                                            | Test setup                                                   |
| --------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| `api/`, `api-donate/` | HTTP requests via `SELF.fetch`, plus visible effects: later responses, logged emails, R2 objects        | Workers Vitest pool with real D1 and R2 (`api/TESTING.md`)   |
| `web/`                | What the user sees and does, by role and visible text (Testing Library), with the network mocked by MSW | `web/src/test-utils.tsx`, `web/src/mocks/`                   |
| `client/core/`        | The `Scenario` DSL, which drives the whole daemon with a fake clock, randomness, platform and API       | `client/core/src/testing/`, `client/core/tests/scenarios.rs` |
| `hash-server/`        | HTTP requests                                                                                           | `hash-server/tests/integration.rs`                           |
| Platform clients      | Each platform's own test project, plus the device → API integration tests in `client/*/scripts/`        | `AGENTS.md`                                                  |
| Contracts             | The shared vectors in `batch-format/vectors/`, checked by both the Rust and TypeScript implementations  | `batch-format/SPEC.md`                                       |

Test an internal function directly only when it is pure and its whole input space is
hard to reach from outside (encodings, time math, layout). It still cites the section
whose behavior it serves.

## Known bugs

When the code doesn't meet the spec and the fix belongs in a later change:

- Write the test for the spec'd behavior anyway.
- TypeScript: use `it.fails(...)` with a `// KNOWN-BUG: <what is wrong>` comment
  directly above. When the code is fixed the test starts failing, so drop `.fails`.
- `it.fails` passes whichever assertion throws, so its body holds only the check that shows
  the bug. If the test needs a setup check first (a status, a precondition), use a normal
  `it` that asserts today's wrong behavior instead, with the `KNOWN-BUG` comment saying what
  the spec wants. It then fails both when the bug is fixed and when something else breaks.
- Rust: add `#[should_panic(expected = "KNOWN-BUG")]` and give the assertion that shows
  the bug a message starting `KNOWN-BUG: <what is wrong>`. A setup failure panics with a
  different message, so it still fails the test. When the code is fixed the test fails
  with "did not panic", so drop the attribute and the message. Keep `#[ignore]` for tests
  that are not meant to run in CI, such as generators.
- `just spec-coverage` lists every `KNOWN-BUG`, so it doubles as the fix list.

## Coverage

`just spec-coverage` prints, for each spec section, how many tests cite it, plus the
uncovered sections and the known bugs.

`scripts/spec-coverage-allow.txt` lists the sections that have no test yet. CI fails
when a new section has no test, and when a listed section gains one (remove its line),
so the list only shrinks. Sections with nothing to test, such as overviews, stay on
the list with a reason.

## Keeping edge cases out of the main code

- Handle edge cases at the boundaries. Validate, clamp and normalize where data comes in
  (request parsing, the batch materializer, settings parsing), so the logic behind it
  only sees valid data. One clamp in one place needs one table test.
- Before adding a branch for an edge case, check whether the data model can make it
  impossible: an enum instead of several booleans, a required field after parsing.
- Write edge cases as rows of a table test (`it.each`, a Rust array of cases), not as
  a new test per case. Adding a case should mean adding a line.

## Workflow

1. Behavior change: update the spec first, then add or change a test that cites it,
   then change the code.
2. Bug fix: add a test that fails without the fix and cites the section it violates. If
   no section covers the case, add the sentence to the spec in the same change.
3. Refactor: the tests should not change. If they have to, they were testing the design;
   rewrite them against the interface.

## Property and mutation testing

Two extra tools that find what example-based tests miss. Neither replaces the rules above:
a property test still cites the spec ID of the sentence it proves, in the same places.

**Property tests** are for a large input space with a crisp invariant, such as "decoding
what was encoded gives the same event", "raising the rollout never turns a flag off for a
user", or "changing any byte of an event fails the hash chain". They run `fast-check`
(TypeScript) or `proptest` (Rust) over generated inputs. Keep case counts modest so a file
runs in a few seconds, and keep them in their own `*.property.test.ts` or `properties.rs`
file. Use an example test, not a property, when the input space is small or the expected
value can only be written by repeating the code under test.

| Package        | Property tests                                                                                                                                         |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `api/`         | `test/feature-flags.property.test.ts` (API-061)                                                                                                        |
| `web/`         | `src/utils/api/crypto.property.test.ts`, `src/utils/cache/sync.property.test.ts`, `src/utils/time.property.test.ts`, `src/utils/risk.property.test.ts` |
| `client/core/` | `tests/properties.rs` (BATCH-002, BATCH-003, BATCH-005, BATCH-006)                                                                                     |
| `hash-server/` | `tests/properties.rs` (HASH-006 to HASH-008)                                                                                                           |

They run with each package's normal test command. When a property fails, the framework
prints a shrunk counterexample. Turn it into a table row of an example test so it stays
fixed, then fix the code.

**Mutation testing** changes the code on purpose (flips a comparison, deletes a statement,
returns a constant) and reruns the tests. A mutant that no test fails is _missed_: that
code can change without any test noticing. It is slow, so it is not part of CI for PRs.
`.github/workflows/mutation.yml` runs it monthly and on demand, one job per tool, and
uploads each report as an artifact.

| Command             | Tool          | Scope                                                                                           |
| ------------------- | ------------- | ----------------------------------------------------------------------------------------------- |
| `just mutants-core` | cargo-mutants | `client/core`: `crypto.rs`, `module/lifecycle.rs`, `module/upload.rs`, `module/upload/batch.rs` |
| `just mutants-hash` | cargo-mutants | `hash-server/src/`, without start-up and logging                                                |
| `just mutants-web`  | Stryker       | `web/src/utils/api/crypto.ts`, `batch-materializer.ts`, `web/src/utils/cache/sync.ts`           |

Install `cargo-mutants` once with `cargo install --locked cargo-mutants`. The scope lives
in `client/core/.cargo/mutants.toml`, `hash-server/.cargo/mutants.toml` and
`web/stryker.config.json` (which narrows the tests with `web/vitest.mutation.config.ts`).
Flags after the recipe name go to the tool, for example
`just mutants-core --file core/src/crypto.rs`. The tools edit source files in place and
restore them, because the tests read `batch-format/vectors/` outside each package, so start
from a clean tree and do not run tests in the same checkout meanwhile. Reports are in
`mutants.out/` (see `missed.txt`) and `web/reports/mutation/index.html`. `api/` is not
mutated: Stryker cannot drive the Workers Vitest pool.

A surviving mutant means one of two things. Either the spec has no sentence that says
what the changed code must do, so add the sentence first, or the spec says it and no test
checks it, so add a test that cites that section. If the mutant changes nothing anyone can
observe (a log message, an equivalent rewrite), skip it, and exclude it in the tool's
config only when it keeps coming back.
