# Styling system overhaul — overview

Four sequential plans to promote `@virtueinitiative/shared-web` from a CSS-only package into a typed Preact component library backed by an expanded token system, and migrate both apps onto it.

## Why

Today the styling setup is vanilla CSS organized into a `shared-web` workspace package consumed by `/web` (Preact + Vite) and `/landing` (Astro). Tokens live in `shared-web/tokens.css`; class families (`.btn`, `.control`, `.card`, `.field`, `.alert-*`, `.badge-*`) live in `shared-web/components/*.css`. Pages string-concatenate those classes onto raw markup — only `Dialog.tsx` is a typed Preact wrapper.

Problems uncovered by exploration:

1. **Tokens are bypassed** — ~30% of page CSS hardcodes hex colors. Three different greens for "online/success" (`--accent #008900`, `#14b86f`, `#22c55e`); danger banners use `#c0392b` instead of `var(--danger)`; shadows like `0 10px 24px #0004` instead of `var(--shadow-md)`.
2. **No scales** for spacing, radius, typography, z-index, or breakpoints. ~10 distinct spacing values, ~6 radii, ~12 font sizes chosen by eye; breakpoints scattered across `420/450/480/720/960px`.
3. **Two parallel button families** — `.btn` (primary/outline/ghost/danger) vs `.control` (flat/icon/is-active) overlap heavily, share no idioms, used arbitrarily.
4. **Two alert systems** — `shared-web/components/feedback.css` `.alert-error/-success` vs `web/src/styles/alerts.css` `.global-alert` toast stack — disjoint markup and visuals.
5. **Dark mode declared three times** in `tokens.css` (`[data-theme="dark"]`, `@media (prefers-color-scheme: dark)`, and `[data-theme="light"]` escape inside the media query) — adding a token means editing three blocks.
6. **No real component layer** — only `Dialog.tsx` is typed; `Button.tsx` is a one-line className forwarder.
7. **Missing primitives** that pages reach around: Input/Textarea/Select/Checkbox/Radio, Tabs (Logs rolls its own), Toast (alerts rolls its own), Spinner, Skeleton, Menu/Dropdown, Tooltip, Avatar.
8. **No namespace separation** — shared `.btn`/`.card`/`.field` live in the same global pool as app-specific `.dashboard-*`/`.logs-*`/`.auth-*`.

## Direction

`shared-web` becomes a typed Preact component library. Each component co-locates `.tsx` + `.css`. Library classes are namespaced with `vi-` so the boundary with app-specific classes is visible in markup. Both apps consume the same package — `/web` imports the typed components everywhere; `/landing`'s Astro pages use them via Preact islands where interactivity matters and via the underlying `vi-*` classes for static markup.

## The four plans

- [`01-foundation.md`](./01-foundation.md) — Build the library, expand tokens, build a `/dev/components` preview page. **No consumer migration.** Both apps continue to work because old class names stay in place; new `vi-*` classes coexist.
- [`02-landing-homepage.md`](./02-landing-homepage.md) — Migrate the landing homepage. Validates the recipe in Astro before the bigger Plan 4 sweep.
- [`03-web-dashboard.md`](./03-web-dashboard.md) — Migrate the `/web` Home (dashboard) page. Validates the recipe in Preact.
- [`04-migrate-rest.md`](./04-migrate-rest.md) — Migrate every remaining page in both apps and delete the legacy unprefixed class definitions. End state: every hex color outside `tokens.css` is gone; every shared class is `vi-*`-prefixed; no parallel button or alert system.

Each plan is independently shippable as its own PR. Plans 2–4 all depend on Plan 1; Plans 2 and 3 are parallelizable; Plan 4 should land last.

## Decisions taken

- Library shape: typed Preact components (not CSS-only, not hybrid).
- Rollout: split into four sequential PRs (one big-bang pass is too risky).
- Extras included: standardized breakpoints, missing primitives, `/dev/components` preview page.
- Extras *not* included (out of scope): stylelint guardrails for token usage. Can revisit after Plan 4 if the manual discipline isn't holding.
