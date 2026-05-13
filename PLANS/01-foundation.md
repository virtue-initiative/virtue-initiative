# Plan 1 — Foundation: token scales + typed component library

> See [`00-overview.md`](./00-overview.md) for the project-wide context behind these plans.

## Context

`@virtueinitiative/shared-web` ships CSS-only today. Tokens exist but are bypassed by ~30% of page CSS; there are no scales for spacing, radius, typography, z-index, or breakpoints; dark mode is declared three times in `tokens.css`; two parallel button families (`.btn` vs `.control`) and two alert systems (`.alert-*` vs `.global-alert`) coexist. Only `Dialog.tsx` is a typed Preact component — every other call site string-concatenates class names onto raw markup.

Plan 1 lands the new system *without touching any existing page*. Both apps keep working because old class names stay in place; new `vi-*` classes and typed components coexist alongside the legacy ones. Plans 2–4 then migrate consumers and delete the legacy.

## Goal

A complete, tested, previewable component library with:

- An expanded token system (spacing, radius, typography, z-index, breakpoints) and dark-mode collapsed to one source of truth.
- Typed Preact components for every primitive listed below, each co-locating `.tsx` + `.css`.
- All library class names namespaced `vi-*`.
- A `/dev/components` preview page in `/web` that renders every variant in light + dark.
- Zero changes to existing page markup.

## 1.1 Expand `shared-web/tokens.css`

Add scales for spacing, radius, typography, z-index, and breakpoints. Collapse dark-mode declarations into a single source of truth.

```css
:root {
  /* spacing — 4px base, t-shirt scale */
  --space-1: 0.25rem; --space-2: 0.5rem; --space-3: 0.75rem;
  --space-4: 1rem;    --space-5: 1.5rem; --space-6: 2rem;
  --space-7: 3rem;    --space-8: 4rem;

  /* radius */
  --radius-sm: 4px; --radius-md: 8px; --radius-lg: 12px; --radius-full: 999px;

  /* typography */
  --text-xs: 0.75rem; --text-sm: 0.875rem; --text-base: 1rem;
  --text-lg: 1.125rem; --text-xl: 1.25rem; --text-2xl: 1.5rem;
  --line-tight: 1.2; --line-normal: 1.5;

  /* z-index */
  --z-base: 1; --z-dropdown: 50; --z-dialog: 100; --z-toast: 200;

  /* breakpoints — used by JS via matchMedia; sm/md/lg media queries documented below */
  --bp-sm: 480px; --bp-md: 720px; --bp-lg: 960px;
}
```

Documented breakpoint contract (comment block in `tokens.css`):

```
@media (min-width: 480px)  → sm
@media (min-width: 720px)  → md
@media (min-width: 960px)  → lg
```

Collapse dark mode: light defaults in `:root`, dark overrides only under `[data-theme="dark"]`. The first-paint scripts in `landing/src/components/ThemeScript.astro` and `web/src/hooks/useDarkMode.ts` already set `data-theme` from `prefers-color-scheme` when no user preference is stored — that's where media-query awareness belongs.

## 1.2 Restructure `shared-web` to ship typed components

```
shared-web/
  package.json          # add preact peer dep + exports map
  tsconfig.json         # new — Preact JSX, ES2022
  tokens.css            # expanded (1.1)
  base.css              # unchanged entry — still imports index.css
  index.css             # imports tokens + each component's .css
  index.ts              # NEW — re-exports components + state + risk
  state.ts              # unchanged
  risk.ts               # unchanged
  components/
    Button/{Button.tsx, Button.css, index.ts}
    IconButton/...      # absorbs .control + .control-icon + is-active
    Card/...            # Card, CardHeader, CardActions, CardGrid
    Field/...           # wraps label + helpText + error around input
    Input/...
    Textarea/...
    Select/...
    Checkbox/...
    Radio/...
    Dialog/...          # port from web/src/components/Dialog.tsx
    Alert/...           # inline variant (replaces .alert-error/-success)
    Toast/{Toast.tsx, ToastProvider.tsx, useToast.ts, Toast.css}
    Badge/...
    Tabs/...
    SegmentedControl/...
    Spinner/...
    Skeleton/...
    Menu/...
    Tooltip/...
    Avatar/...
    index.ts            # barrel
```

No build step — Vite (used by both apps) compiles `.tsx` directly via the workspace symlink.

## 1.3 Namespace + reconciliation

All library classes get a `vi-` prefix: `.vi-btn`, `.vi-card`, `.vi-field`, `.vi-badge`, etc. **Old names stay defined** in `shared-web/components/*.css` so existing pages keep working — they're deprecated, not removed. Plan 4 deletes them.

Reconciliations done inside the new components:

- `Button` unifies `.btn` and `.control`. Props: `variant: 'primary' | 'outline' | 'ghost' | 'danger' | 'flat'`, plus `size`, `disabled`, `type`. Standard button.
- `IconButton` is icon-only with `isActive` prop (replaces `.control-icon` + `.is-active`).
- `Alert` is inline (replaces `.alert-error` / `.alert-success`).
- `Toast` + `ToastProvider` + `useToast()` is the global stack (replaces `.global-alert`). Provider mounted in Plan 1 from `web/src/index.tsx`; old `.global-alert` markup left in place until Plan 4.

## 1.4 Component preview page

Add `web/src/pages/Dev/Components/index.tsx`, registered behind `import.meta.env.DEV` (and a `?dev=1` query toggle so it's reachable from production preview builds too). Renders every component with every variant in light + dark side-by-side, plus the full token palette. Source of truth for what exists; visual-regression bait throughout Plans 2–4.

## 1.5 Astro integration check

Verify `@astrojs/preact` is in `landing/astro.config.mjs`. If absent, add it. No consumer changes yet — this just enables Plan 2.

## Critical files

**New:**
- `shared-web/index.ts`
- `shared-web/tsconfig.json`
- Every component folder under `shared-web/components/`
- `web/src/pages/Dev/Components/index.tsx`

**Modified:**
- `shared-web/package.json` — Preact peer dep + exports map
- `shared-web/tokens.css` — scales + dark-mode collapse
- `shared-web/index.css` — re-imports new component CSS
- `landing/astro.config.mjs` — if `@astrojs/preact` missing
- `web/src/index.tsx` — mount `<ToastProvider>`, register Dev/Components route

**Untouched:** every existing page, every existing `style.css`, every existing call site of `.btn`/`.card`/`.field`/`.alert-*`/`.global-alert`/`.control`. (That's intentional — Plan 1 must not change pixels on any user-facing page.)

## Verification

1. `pnpm -w typecheck` — component props type-check cleanly.
2. `pnpm --filter web dev` and `pnpm --filter landing dev` — both apps boot and behave identically to pre-change for normal users.
3. Open `/dev/components` in `/web` — every variant renders in both `data-theme="light"` and `data-theme="dark"`. Toggle via `<ThemeButton>` in the header. Token palette renders all `--space-*`, `--radius-*`, `--text-*`, `--z-*` values labeled.
4. Manual smoke of `/web` (Home, Auth, Settings, Logs) and `/landing` (home, a blog post, downloads) — visually identical to pre-change.
5. `pnpm --filter web build && pnpm --filter landing build` — production builds succeed.
6. `pnpm --filter web test` — existing Vitest suite passes.
