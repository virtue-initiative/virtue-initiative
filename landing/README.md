# Landing site

The Astro site at https://virtueinitiative.org: the home page, downloads, help pages, blog and
legal pages. It also publishes the feeds the client apps update from.

## Run it

`./scripts/launch.sh` from the repo root starts it with the rest of the stack. Alone:

```bash
just landing-dev
```

Starting the dev server first refreshes the release data `/download` shows
(`scripts/fetch-release-data.mjs`, which reads GitHub Releases). Run
`bun run sync-release-data` to refresh it by hand.

## Where things are

- `src/pages/`: routes.
- `src/content/help/`: the help pages, in Markdown, for people using Virtue. Developer docs live
  in the repo's `docs/` instead. Each page's frontmatter needs a `title`, a one-line
  `description` (shown on topic pages and in search), a `topic` and an `order`; the topics are
  listed in `src/help/navigation.ts`, and a page with bad frontmatter fails the build. The
  topic is not part of the URL, so pages can be regrouped without breaking the links the apps
  use. Follow the copy style checklist in the root `AGENTS.md`, and use the names the apps' UI
  uses.
- `src/content/download-instructions/`: per-platform install steps shown on `/download`.
- `src/content/blog/`: release notes and posts.
- `scripts/build-appcast.mjs`: writes `appcast.xml`, the macOS Sparkle feed
  (`client/mac/RELEASING.md`).
- `scripts/build-android-update.mjs`: writes `android-update.json`, the Android update feed
  (`client/android/DESIGN.md`).

## Check and deploy

```bash
just landing-typecheck
just landing-build
```

`bun run build` also runs Pagefind, which indexes the help pages into `dist/pagefind/` for the
search box. There is no index under `astro dev`, so check search with `bun run build` and
`bunx astro preview`.

Both need `GITHUB_TOKEN` set (any value works locally, for example `GITHUB_TOKEN=stub`)
because they prepare release data first. `bun run format:check` checks formatting.

CI deploys on pushes to `staging` and `main` (`.github/workflows/deploy-landing.yml`). By hand:
`bun run deploy:staging` or `bun run deploy:prod`.
