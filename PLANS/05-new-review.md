# Styling overhaul — first-pass cleanup

## Context

Branch `new-components` has completed a first pass of the four-plan styling overhaul (`PLANS/00-overview.md`). The typed Preact component library exists, both apps are functionally migrated, and almost every legacy class name has been replaced. This plan finishes the loose ends so Plan 4's verification greps (`git grep -nE "#[0-9a-fA-F]{3,8}\b"` and `git grep` for legacy class names) return empty across `web/src/` and `landing/src/`.

Scope is intentionally narrow — these are the residuals an audit surfaced, not new features. No visual changes for users.

## Findings summary

| Area | Status | Residuals |
|---|---|---|
| `shared-web` components (20 typed) | complete | none |
| `shared-web/tokens.css` scales | complete | dark mode declared twice (lines 72–86 + 88–104) |
| `shared-web/components/*.css` legacy stubs | mostly stripped | 4 near-empty stub files + their imports |
| `/web` page migrations | complete | 4× `card-name`/`card-meta` in Home; 7 hex/rgba in Logs CSS; 1 inline style; `GLOBAL_ALERT_EVENT` bridge |
| `/landing` page migrations | complete | 4× `#fff` in `downloads.css` + `help-sidebar.css`; `@astrojs/preact` never installed |
| Legacy wrappers (`web/src/components/{Button,Dialog}.tsx`, `web/src/styles/alerts.css`) | already deleted | n/a |

## 1. `/web` cleanup

### 1.1 Tokenize remaining hex/rgba in Logs

| File:line | Current | Replace with |
|---|---|---|
| `web/src/pages/Logs/batch.css:3,20,25,30` | `color: #fff;` (4×) | `color: var(--surface);` (semantic — these are text on a colored chip surface; verify in light + dark on `/dev/components`) |
| `web/src/pages/Logs/layout.css:152` | `color: #fff;` | `color: var(--surface);` |
| `web/src/pages/Logs/layout.css:156` | `background: #fff;` | `background: var(--surface);` |
| `web/src/pages/Logs/layout.css:541` | `background: rgba(0, 0, 0, 0.5);` | `background: var(--overlay);` (already `#0006` in tokens) |

Test both themes after each swap — `var(--surface)` flips to dark while `#fff` stays white, so the visual diff may reveal that one or two should actually be a literal `white` (a "color on a colored chip" context). Use `/dev/components` Token Palette section as a reference.

### 1.2 Promote `card-name` / `card-meta` to `vi-` namespace

Pure rename — no behavior change.

- `shared-web/components/cards.css` — rename `.card-name` → `.vi-card__name` and `.card-meta` → `.vi-card__meta`. Drop the inline "no vi-* equivalents exist" comment.
- `web/src/pages/Home/index.tsx:394, 480, 629` — `card-name` → `vi-card__name`.
- `web/src/pages/Home/index.tsx:634` — `card-meta` → `vi-card__meta`.
- Update any CSS selectors in `web/src/pages/Home/style.css` (or elsewhere) that reference `.card-name` / `.card-meta`.
- Update `/dev/components` Card section if it references either.

### 1.3 Remove inline static style

`web/src/pages/Logs/LogsList.tsx:69` — replace `style="margin-left: 0.5rem"` with a sibling utility class on `.logs-device` (e.g. `logs-device--indented`) whose rule lives in `Logs/layout.css` and uses `margin-left: var(--space-2)`.

### 1.4 Remove `GLOBAL_ALERT_EVENT` bridge

The bridge in `web/src/utils/toast.ts` dispatches a `CustomEvent` that `web/src/index.tsx` listens for and forwards to `useToast()`. Inline this:

- Convert `web/src/utils/toast.ts` to a tiny module that owns a reference to `useToast`'s `push` callback. Provide an `initToast(push)` function called once from a top-level component (e.g. the same place `<ToastProvider>` is mounted in `web/src/index.tsx`) and a `pushToast(message, kind)` that delegates to it.
- Remove the `addEventListener(GLOBAL_ALERT_EVENT, …)` block in `web/src/index.tsx:96-98` (and the import on line 23).
- Delete `web/src/events.ts` (verify no other consumers first: `git grep GLOBAL_ALERT_EVENT web/src/`).
- All existing callers of `pushToast` keep working — the function signature doesn't change.

Alternative if any caller is itself inside a React tree: have it call `useToast()` directly instead of going through `utils/toast.ts`. Decide per call site during implementation.

### 1.5 Add first-paint theme script to `/web`

Currently `useDarkMode.ts` is an async hook — a user with system dark preference and no stored theme sees a light flash before the hook fires. Plan 1 wanted dark mode collapsed in `tokens.css`, which requires this.

- Add an inline `<script>` to `web/index.html` (head, before any module script) that mirrors `landing/src/components/ThemeHeadScript.astro:2-49`: read `shared-state-local` / `shared-state` from `localStorage`, fall back to `matchMedia("(prefers-color-scheme: dark)")`, then synchronously set `document.documentElement.setAttribute("data-theme", …)` and `style.colorScheme`.
- Reuse the same logic — copy and adapt; or extract to a shared `.js` file under `shared-web/` if both apps can share it (only if the storage keys match — verify first).
- Once this is in place, the `@media (prefers-color-scheme: dark)` block at `shared-web/tokens.css:88-104` becomes safe to delete. Do that next.

## 2. `/landing` cleanup

### 2.1 Add `@astrojs/preact` via official CLI

```bash
cd landing && npx astro add preact
```

This registers the integration in `landing/astro.config.mjs` and installs the dep into `landing/package.json` + `landing/package-lock.json`. Because Astro prerenders Preact components to HTML at build time by default, `.astro` pages can then import `<Button>` / `<Card>` / `<Alert>` etc. from `@virtueinitiative/shared-web` and emit zero client JS unless a `client:*` directive is added.

No page changes required in this PR — landing still works as static markup. This just unblocks future island work without setup friction.

### 2.2 Tokenize remaining `#fff` in landing styles

| File:line | Current | Replace with |
|---|---|---|
| `landing/src/styles/downloads.css:5` | `color: #fff;` (dev-warning-banner) | `color: var(--surface);` — verify on the warning banner background |
| `landing/src/styles/help-sidebar.css:147,165,174` | `color: #fff;` (3×, fab + active sidebar links) | `color: var(--surface);` — verify in light + dark, the sidebar overlay forces a dark background so this may legitimately want to stay white; if so use a documented exception |

If `var(--surface)` doesn't visually match (because the surface flips white→dark with theme), keep the literal `white` but add a `/* literal: forced-dark overlay context */` comment so the audit grep can be filtered for documented exceptions. Better: add a `--text-on-dark` token to `tokens.css` if this comes up more than once.

### 2.3 ThemeHeadScript.astro hex values — document only

`landing/src/components/ThemeHeadScript.astro:53-70` has 8 hardcoded hex literals (`#f9f9f7`, `#1a1a1a`, `#111713`, `#e6e3df`) inside an inline `<style is:inline>`. These must stay hardcoded — they paint before `tokens.css` loads. Add a comment at the top of the `<style>` block:

```html
<!-- These four colors must stay in sync with tokens.css :root and [data-theme="dark"] for --bg and --text. They cannot reference CSS custom properties because they paint before tokens.css loads. -->
```

If `/web` reuses the same first-paint pattern in §1.5, mirror this comment there.

## 3. `shared-web` cleanup

### 3.1 Collapse dark mode in `tokens.css`

After §1.5 lands, delete `shared-web/tokens.css:88-104` (the `@media (prefers-color-scheme: dark)` block). Single source of truth: `:root` for light defaults, `[data-theme="dark"]` for dark.

### 3.2 Delete empty legacy CSS stubs

These files are 1-line "removal" comments and provide no rules:

- `shared-web/components/buttons.css` — delete
- `shared-web/components/feedback.css` — delete
- `shared-web/components/segmented-control.css` — delete
- `shared-web/components/dialog.css` — delete

`shared-web/components/forms.css` (10 lines, autofill overrides) and `shared-web/components/cards.css` (now contains `vi-card__name`/`vi-card__meta` after §1.2) stay.

Update `shared-web/index.css` to remove the four `@import` lines for the deleted stubs.

## 4. Critical files

**Modified:**
- `shared-web/tokens.css` — delete @media dark block (§3.1)
- `shared-web/index.css` — drop 4 `@import`s (§3.2)
- `shared-web/components/cards.css` — rename `.card-name` / `.card-meta` (§1.2)
- `web/index.html` — add inline first-paint theme script (§1.5)
- `web/src/pages/Home/index.tsx` — `card-name` × 3, `card-meta` × 1 → vi-* (§1.2)
- `web/src/pages/Logs/batch.css` — 4 `#fff` → tokens (§1.1)
- `web/src/pages/Logs/layout.css` — `#fff` × 2 + `rgba(...)` → tokens (§1.1)
- `web/src/pages/Logs/LogsList.tsx` — drop inline style (§1.3)
- `web/src/pages/Logs/layout.css` — add new `.logs-device--indented` rule (§1.3)
- `web/src/utils/toast.ts` — replace event dispatch with direct push (§1.4)
- `web/src/index.tsx` — drop GLOBAL_ALERT_EVENT listener + import; wire `initToast` after `<ToastProvider>` mounts (§1.4)
- `landing/astro.config.mjs` — auto-edited by `astro add preact` (§2.1)
- `landing/package.json` — auto-edited by `astro add preact` (§2.1)
- `landing/src/styles/downloads.css` — `#fff` → token or documented (§2.2)
- `landing/src/styles/help-sidebar.css` — 3× `#fff` → token or documented (§2.2)
- `landing/src/components/ThemeHeadScript.astro` — add comment (§2.3)

**Deleted:**
- `shared-web/components/buttons.css`
- `shared-web/components/feedback.css`
- `shared-web/components/segmented-control.css`
- `shared-web/components/dialog.css`
- `web/src/events.ts` (after verifying no other consumers — §1.4)

## 5. Existing utilities to reuse

- `shared-web/components/Toast/useToast.ts` — `useToast()` hook + `ToastProvider` (already wired in `web/src/index.tsx:211`). The §1.4 refactor exposes its `push` to non-component callers via a module-level reference.
- `landing/src/components/ThemeHeadScript.astro` — source pattern for the new `/web` first-paint script in §1.5. Same `STORAGE_KEYS = ["shared-state-local", "shared-state"]` and `matchMedia` fallback.
- `shared-web/state.ts` — `getState()`, `preferredTheme()`. The first-paint script may either parse `localStorage` directly (matching the landing pattern) or import from here if the inline script can resolve the module (it cannot — inline scripts can't `import`). Plan: parse `localStorage` directly.
- `web/src/pages/Dev/Components/index.tsx` — visual reference for verifying token swaps in §1.1 and §2.2 render correctly in light + dark.

## Verification

1. `cd web && npx tsc --noEmit` — clean. (Repo uses per-app npm, no workspace.)
2. `cd web && npm run build && cd ../landing && npm run build` — both production builds succeed.
3. `cd web && npm test` — existing Vitest suite passes.
4. `git grep -nE "#[0-9a-fA-F]{3,8}\b" web/src/ shared-web/components/` — empty (after §1.1 and §3.2).
5. `git grep -nE "#[0-9a-fA-F]{3,8}\b" landing/src/` — only hits are the 8 documented hex literals in `ThemeHeadScript.astro` (per §2.3). No other matches.
6. `git grep -nE '\b(card-name|card-meta|alert-error|alert-success|global-alert)\b' web/src/ landing/src/` — empty.
7. `git grep -n GLOBAL_ALERT_EVENT web/src/` — empty.
8. Manual smoke `/web`:
   - Hard refresh in a private window with system dark preference set, no localStorage — page renders **dark on first paint** (no flash). Confirms §1.5.
   - Toggle theme via `<ThemeButton>` — switches correctly in Home, Auth, Settings, Logs.
   - Trigger a toast (force a sync error, or invite a partner) — appears via `<ToastProvider>`. Confirms §1.4.
   - Walk Logs list, gallery, batch, lightbox — visuals identical to pre-change in both themes. Confirms §1.1.
9. Manual smoke `/landing`:
   - Home, downloads, help sidebar (mobile drawer open + active link), a blog post — visuals identical in both themes. Confirms §2.2.
   - Hard refresh with system dark — no FOUC (unchanged behavior, but re-verify after touching ThemeHeadScript comment).
10. `/dev/components` — every variant still renders cleanly in both themes (sanity check that nothing in `shared-web` regressed).
