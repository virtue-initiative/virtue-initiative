# Finding unused CSS and TSX

Two tools, one for each side. Neither needs to be installed permanently — run via `npx`.

## TSX / TypeScript: [Knip](https://knip.dev)

Finds unused files, exports, dependencies, types, and class members across `web/`, `landing/`, `shared-web/`. Successor to `ts-prune` (which is in maintenance mode).

```sh
# Run once, no install:
npx knip                 # from web/, landing/, or shared-web/
npx knip --include files # only unused files
npx knip --fix           # auto-remove unused exports (review the diff!)
```

First run will offer to create `knip.json`. For monorepo-style layout here, run per-package rather than at repo root. Knip auto-detects Vite/Astro/Preact.

Caveats:

- Dynamically-imported components may show as false positives — add them to `ignore` in `knip.json`.
- Don't run `--fix` on crypto / wire-format code without manual review (see `CLAUDE.md` cross-component contracts).

## Plain CSS: [PurgeCSS](https://purgecss.com)

This repo uses **plain CSS files** (not CSS Modules), so `check-unused-css` doesn't apply. PurgeCSS scans content files (`.tsx`, `.astro`, `.html`) for class names and reports/removes selectors not found.

```sh
# Dry-run report (recommended first):
npx purgecss \
  --css web/src/**/*.css shared-web/*.css \
  --content 'web/src/**/*.{tsx,ts,html}' 'shared-web/**/*.{tsx,ts}' \
  --output /tmp/purged
# Then diff /tmp/purged against the originals to see what would be removed.
```

Caveats:

- Dynamically-built class names (`` `btn-${variant}` ``) look unused — add a `safelist` in a `purgecss.config.js` if needed.
- Astro/Preact attribute-bound classes are detected, but verify before deleting.
- Global resets, `:root` tokens, and `@keyframes` referenced by name are usually safe, but spot-check.

## Alternative for stricter CSS Modules adoption

If we ever migrate to `*.module.css`, switch to [`check-unused-css`](https://github.com/malinindev/check-unused-css) — zero-config, TS-aware, much lower false-positive rate than PurgeCSS.
