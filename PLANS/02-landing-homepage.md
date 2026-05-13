# Plan 2 — Migrate the landing homepage

> See [`00-overview.md`](./00-overview.md) for the project-wide context. Depends on [`01-foundation.md`](./01-foundation.md).

## Context

After Plan 1 lands, the new typed Preact components and `vi-*` namespaced classes exist in `shared-web` alongside the legacy `.btn`/`.card`/etc. classes. No consumer has migrated yet. Plan 2 picks the smallest, lowest-risk consumer — the landing homepage — and converts it to the new system. The point isn't the homepage itself; the point is to validate the recipe in Astro before committing to it across the rest of `/landing` in Plan 4.

## Goal

The landing homepage renders entirely from typed components and `vi-*` classes, with all hex colors and ad-hoc spacing values replaced by tokens. Other landing pages stay on the legacy class layer until Plan 4.

## Scope

- `landing/src/pages/index.astro` — the page itself.
- Any homepage-only partials it pulls in.
- Sections of `landing/src/styles/global.css` that *only* the homepage uses.

Out of scope: blog index, blog posts, downloads, help-sidebar, tables. CSS shared with other pages stays unchanged — those move in Plan 4.

## 2.1 Component swaps

- Buttons / CTAs → `<Button>` (Preact island if interactive) or `vi-btn` class on a static `<a>` for plain links.
- Feature blocks / cards → `<Card>` or `vi-card` class on plain markup.
- Any inline form on the homepage → `<Field>` + `<Input>` + `<Button>`.
- Any inline alerts/badges → `<Alert>` / `<Badge>`.

Decide per element whether it needs an island (interactivity → component) or just the underlying class (static markup → `vi-*` class on raw HTML). For a marketing page, default to classes; reserve islands for things that genuinely need JS.

## 2.2 Token swaps in homepage CSS

Walk every `.css` file scoped to the homepage. Replace:

- hex colors → `var(--*)` semantic tokens (or add a new token if a need is genuine).
- ad-hoc rem spacing → `var(--space-*)`.
- ad-hoc radii → `var(--radius-*)`.
- ad-hoc font sizes → `var(--text-*)`.
- ad-hoc breakpoints → the documented `sm/md/lg` queries from Plan 1.

Leave styles that other landing pages also use untouched — those move in Plan 4.

## Critical files

- `landing/src/pages/index.astro` — the page.
- `landing/src/styles/global.css` — only the homepage-relevant sections; mark others "Plan 4."
- Any homepage-only partials.

Untouched: every other page in `/landing`, all of `/web`.

## Verification

1. `pnpm --filter landing dev` — homepage renders correctly in light + dark, desktop + mobile (test all three breakpoints: <480px, 480–720px, >960px).
2. `pnpm --filter landing build` — production build succeeds.
3. Visual diff against a pre-Plan-2 screenshot — spacing/colors/typography preserved (or deliberately improved with a noted reason).
4. `git grep -nE "#[0-9a-fA-F]{3,8}\b" landing/src/pages/index.astro` — no hex literals remain in the homepage source.
5. Other landing pages (blog index, a blog post, downloads, help) still render correctly — they're still on the legacy class layer, which Plan 1 deliberately preserved.
6. `/dev/components` in `/web` still renders cleanly (sanity check that nothing in `shared-web` regressed).
