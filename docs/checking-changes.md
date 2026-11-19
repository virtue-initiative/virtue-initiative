# Checking a change

How to find out quickly whether a change works. [`TESTING.md`](../TESTING.md) says how to
_write_ tests; this page says what to _run_, in what order, and how to see the change working
outside the tests.

## The loop

Start with the narrowest check that can fail, and widen once it passes.

1. **The tests for the behavior you're changing.** `just tests-for CORE-002` lists every test
   that cites a spec ID, grouped by file, with a command that runs just those. Run them before
   you change the code: a new or changed test should fail first.
2. **The area's own tests**, with the commands in the table below, while you iterate.
3. **Everything the branch touches**, before you say you're done: `just check`. It works out
   which areas changed since `staging` (including uncommitted files), runs the same commands
   CI runs for each, in parallel, and prints one line per passing command and the end of the
   output for a failing one. `just check --list` shows what it would run; `just check web api`
   runs named areas. A branch touching the API, web app, hash server and Rust core takes about
   3 minutes.
4. **See it run**, for anything a user would notice (see below).

## Commands per area

Times are warm runs on a Linux dev machine, after the first build.

| Area           | One file or test                                                  | Whole suite                                                                |
| -------------- | ----------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `api/`         | `bun run test -- test/devices.test.ts` (5s)                       | `bun run test` (40s); `typecheck` 2s                                       |
| `web/`         | `bun run test -- src/utils/api/crypto.test.ts` (5s)               | `bun run test` (12s); `typecheck` 3s                                       |
| `client/core/` | `cargo test -p virtue-core --features testing --test tamper` (5s) | `cargo test -p virtue-core --features testing --tests` (50s); `clippy` 15s |
| `hash-server/` | `cargo test --test integration`                                   | `cargo test`                                                               |
| Specs and docs |                                                                   | `just spec-check`, `just doc-check` (under a second each)                  |

- **Filter TypeScript tests by file, not by name.** `bun run test -- -t API-038` still starts
  every test file's Worker and takes about 24s in `api/`; naming the file takes about 5s.
- **Rust tests cite spec IDs in comments**, so `cargo test CORE-002` matches nothing. Use
  `just tests-for` to get the test names or the module to filter on.
- **Core unit tests need `--features testing`.** The `Scenario` DSL in
  `client/core/src/testing/` (fake clock, randomness, platform and API) is the quickest way to
  check daemon behavior, and quicker than running a real client.
- **Always name the Rust package** (`-p virtue-core`). A bare `cargo test` in `client/` tries
  to build crates for other OSes.

## Seeing it run

Tests pass on code that is still wrong in ways a user would see straight away. For a change to
something a user sees, also run it:

- **Web app and landing site:** start the stack with `./scripts/launch.sh` in the background,
  writing its output to a file, then drive the page with `bunx playwright-cli` (the
  `playwright-cli` skill). Find elements by role and visible text. `just seed` gives the
  `dev@dev.com` account a week of activity, so the Logs and Report pages have data. See
  "Driving the web app in a browser" in [`development.md`](development.md).
- **Emails:** with `EMAIL_DELIVERY_MODE=log` in `api/.dev.vars`, outgoing emails are printed
  to the `[api]` lines of the `launch.sh` log.
- **Linux client:** the `virtue-login` skill builds the `.deb` against the local API,
  installs it and signs in; `virtue status --advanced` shows the result. For the whole path
  from daemon to database, `bun client/linux/scripts/integration-test.ts` runs the real
  daemon under Xvfb against a fresh local API.
- **Other platforms:** `just check` lists the macOS, Windows, iOS and Android checks it can't
  run on this machine, with where to run them instead (`just mac-remote`, `just ios-remote`,
  `client/windows/scripts/remote-windows-build.sh --mode smoke`). Each platform's README covers
  running it.

## Reporting what you checked

When you finish, say which commands you ran and what they covered, not just "tests pass".
Name anything you couldn't run, such as a platform's checks that need macOS or Windows, so the
reviewer knows what CI or a person still has to cover. For a UI change, attach a screenshot to
the PR.

## Mistakes to avoid

- **Don't weaken a failing test to make it pass.** If the test is right and the fix is out of
  scope, mark it `KNOWN-BUG` as [`TESTING.md`](../TESTING.md) describes.
- **Write the test from the spec, not from your code.** A test written after the code tends to
  check what the code does, wrong or not. Write it first, or at least check that it fails
  without your change.
- **Check the behavior a user sees.** A test that checks a helper was called can pass while the
  page or the response is still wrong.
- **Don't pipe `launch.sh` into `head` or `tail`.** It wedges the API.
