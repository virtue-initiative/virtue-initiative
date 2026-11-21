# Releasing

This page explains how a change gets from a merged PR to the servers and to people's devices, for
every part of the system at once. Everything merges into `staging`, which deploys
the servers' staging copies and publishes test builds of the apps, and a release is a PR from
`staging` into `main`, which deploys production and publishes the stable apps. The signing,
store and pipeline details for each app are in its own `RELEASING.md` (`client/mac/`,
`client/windows/`, `client/ios/`), and [`ci-checks.md`](ci-checks.md) lists what CI runs.

## What each branch does

A push to `staging` or `main` deploys or publishes everything that changed, with no further step,
so merging is the release action. Each merge sets off this:

| Part          | A push to `staging`                                              | A push to `main`                                           |
| ------------- | ---------------------------------------------------------------- | ---------------------------------------------------------- |
| `api`, `web`  | The staging Workers (`deploy.yml`)                               | The production Workers                                     |
| `api-donate`  | **Production**, since it has no separate release                 | Production                                                 |
| `landing`     | **Both** the production and staging sites (`deploy-landing.yml`) | Both sites                                                 |
| `hash-server` | `staging-virtue-hash` (`deploy-hash-server.yml`, HASH-018)       | `virtue-hash`                                              |
| Client apps   | Dev builds, tagged `<VERSION>-dev`, that talk to the staging API | Stable builds, tagged `<VERSION>`, that talk to production |

Watch the two bold cells: a change to `api-donate/` or to the help pages goes live in production
as soon as it lands on `staging`.

Every app build gets its channel from the branch (`virtue_release_channel` in
`client/scripts/version.sh`), and `client/core/build.rs` uses the same channel to bake in which
API the app talks to, so dev builds use staging data. Each platform keeps the two kinds of build
apart in its own way:

- **Linux:** both are `.deb` files on GitHub Releases, dev builds as prereleases. A dev install
  updates to the newest prerelease, and a stable install to the newest full release
  (`client/linux/DESIGN.md`).
- **macOS:** one Sparkle feed serves both, and only dev builds opt into its `dev` items
  (`client/mac/RELEASING.md`).
- **Windows:** `staging` goes to the Store's "Staging" package flight and `main` to the public
  listing (`client/windows/RELEASING.md`).
- **Android:** both are APKs on GitHub Releases, but only stable builds update themselves
  (`client/android/DESIGN.md`).
- **iOS:** `staging` goes to an opt-in TestFlight group and `main` to the public beta group
  (`client/ios/RELEASING.md`).

## Versions

Every app shares one version, kept in `client/version.properties`: `VERSION`, plus the build
numbers Android and Apple each want (`ANDROID_VERSION_CODE`, `APPLE_BUILD_NUMBER`). After editing
it, run `client/scripts/update-version.sh`, which copies it into every file that hardcodes it. A
PR into `main` must raise all three, and `version-check.yml` fails it otherwise. On top of the
version, each build gets a label of the form `<tag>-<date>-<commit>` (`virtue_build_label`), so
two dev builds of the same version can still be told apart.

## A release, step by step

We track each release in a GitHub issue titled "Release x.y.z". The issue is a checklist of the
work the release needs, so a PR goes on it when it needs something done at release time: a secret
to set, a migration to run, or something to test by hand. (Most PRs need none of those.) The
release PR is where everything shipping gets listed. A release goes like this:

1. **Bump the version** on `staging`, as above.
2. **Do what `staging` needs as each PR lands.** Merging to `staging` deploys there, so a new
   secret or migration has to be set on staging before the PR that needs it merges.
3. **Test the dev builds** on every platform: sign in, check that monitoring starts, and work
   through the manual tests on the issue.
4. **Open the release PR** from `staging` into `main`, titled "Release x.y.z" and listing the PRs
   it ships. Its description needs a `# What to Test (iOS)` section, because CI reads it and sends
   it to TestFlight testers (`testflight-notes-check.yml`).
5. **Before merging, prepare production.** Merging deploys the production API, and the deploy does
   **not** run D1 migrations, so run `bun run db:migrate:prod` from `api/` and set any new
   production secrets first. Otherwise the new code meets the old schema.
6. **Merge, then check.** Watch the deploys and the apps' release jobs, try whatever the issue says
   to try on production, and keep an eye on the logs for errors for a while.
7. **Publish the release notes.** They are a post in `landing/src/content/blog/`, written by a
   person like all of the site's copy.

The migration step is the one most likely to hurt if it is missed. Because the migration runs
first, there is a stretch where the new schema serves the old code, so a migration has to work
with the code that is already running. Adding a table, or a column with a default, is safe.
Renaming or dropping one breaks the running code, so split it over two releases: one that stops
using it, and a later one that removes it.

## When something goes wrong

Undoing a release is a manual job. The Workers can be put back by deploying a known-good commit with `bun run deploy:prod` from the
Worker's folder (see "Deployment workflows" in [`development.md`](development.md)), as long as the
database still suits that code. A migration cannot be undone automatically, which is one more
reason to keep each one additive. App builds cannot be pulled back once people have installed
them (the dev and stable update channels will hand the next build out as soon as it exists), so a
bad app release is fixed by a new one with a higher version.
