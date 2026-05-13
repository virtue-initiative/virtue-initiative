# Plan 4 — Migrate everything else and delete legacy

> See [`00-overview.md`](./00-overview.md) for the project-wide context. Depends on [`01-foundation.md`](./01-foundation.md), [`02-landing-homepage.md`](./02-landing-homepage.md), and [`03-web-dashboard.md`](./03-web-dashboard.md).

## Current state summary (as of Plan 2 completion)

Use this as a quick orientation before starting. Run the hex grep in Verification step 4 to confirm nothing has shifted.

### What is done

- **Plan 1 (foundation)**: `shared-web` has all `vi-*` component CSS and TSX. Tokens are in `tokens.css`. Legacy unprefixed rules still exist in parallel (not yet deleted).
- **Plan 2 (landing homepage)**: `landing/src/pages/index.astro` and `landing/src/styles/home.css` are fully migrated — buttons use `vi-btn vi-btn--*`, privacy card uses `vi-card`, all hex literals removed, breakpoints aligned to 480/720/960px. `--radius-xl: 18px` was added to `shared-web/tokens.css`.
- **Plan 3 (web Home)**: `web/src/pages/Home/` is mostly migrated. `<Card>`, `<Button>`, `<Badge>`, `<Dialog>`, `useToast()` are all used. **One residual**: `alert-error` class still at `Home/index.tsx:105` — not yet moved to `<Alert>` or `useToast()`.

### What is NOT done (exact files and counts)

**`/web` — legacy class occurrences:**

| File | Legacy hits | Notes |
|---|---|---|
| `web/src/pages/Auth/index.tsx` | 10 | `card`, `field`×4, `alert-error/success`×3, `btn`×2, `segmented-control`×3 |
| `web/src/pages/Settings/index.tsx` | 17 | `card`×3, `field`×5, `alert-error`×3, `btn`×6 |
| `web/src/pages/Logs/index.tsx` | 5 | `btn btn-ghost btn-sm`×4, `alert-error`×1, `segmented-control`×3 |
| `web/src/components/Header.tsx` | 3 | `control control-icon`×2, `control`×1 |
| `web/src/components/ThemeButton.tsx` | 1 | `control control-icon` |
| `web/src/pages/Home/index.tsx` | 1 | `alert-error` at line 105 (leftover from Plan 3) |

**`/web` — hex/rgba literals remaining:**

| File | Values |
|---|---|
| `web/src/pages/Auth/style.css:51-53` | `#c0392b`, `#ffffff` (dev warning banner) |
| `web/src/pages/Settings/style.css:48` | `rgba(255,255,255,0.02)` (dark-mode-only bg — light-mode bug) |
| `web/src/pages/Logs/layout.css:152,156,176,180` | `#fff`×2, `#22c55e` (online dot), `#9ca3af` (offline dot) |
| `web/src/pages/Logs/batch.css:3,20,24,25,30` | `#fff`×4, `#f57c00` (warning orange) |
| `web/src/styles/alerts.css:10` | `#0004` box-shadow |

**`/landing` — hex literals remaining:**

| File | Values |
|---|---|
| `landing/src/styles/downloads.css:2-5` | `#c0392b`, `#922b21`, `#ffffff` (danger button) |
| `landing/src/styles/help-sidebar.css:91,96,147,165,174` | `#fff`×5 (active link color) |
| `landing/src/styles/help-sidebar.css:146,173` | `rgba(0,0,0,0.18/0.22)` (drawer shadows) |
| `landing/src/styles/help-sidebar.css:186` | `rgba(15,23,42,0.42)` (overlay) |
| `landing/src/components/Header.astro:23` | `class="btn btn-primary"` (shared header, every page) |

### Gotchas discovered in Plan 2 that also apply here

1. **Breakpoint alignment** — The pre-existing breakpoints were not aligned with the documented `480/720/960px` contract. `max-width: 900px` was being used where `max-width: 959px` (below lg) belongs, and `max-width: 640px` where `max-width: 719px` (below md) belongs. Check every media query you touch.

2. **`vi-card` as a base class** — When adding `vi-card` to an element that has custom CSS overrides, the element's own rule only needs the *delta* from `vi-card`'s defaults (`background: var(--surface)`, `border: 1px solid var(--border)`, `border-radius: var(--radius-lg)`, `padding: 1.25rem`). The redundant `border` line was removed from `.privacy-card` because `vi-card` already provides it. Apply the same pattern in `/web`.

3. **`rgba(255,255,255,0.02)` in Settings/style.css:48** is a known dark-mode-only value that doesn't flip for light mode — replacing it with `var(--surface)` or `var(--bg-subtle)` is also a bug fix, not just a token swap.

4. **The `global-alert` system is larger than a single call site** — `web/src/styles/alerts.css` has ~115 lines defining `.global-alert`, `.global-alert-stack`, `.global-alert-close`, `.global-alert--closing`, and their responsive variants. `web/src/index.tsx` renders the entire stack (~40 lines of JSX starting at line 235). Migrating this to `useToast()` means: (a) remove the JSX block in `index.tsx` — `ToastProvider` is already wired at line 368 and renders its own `<ToastList>`; (b) delete `web/src/styles/alerts.css`; (c) remove its `@import` from `web/src/style.css:5`; (d) change callers of the `GLOBAL_ALERT_EVENT` custom event (`web/src/events.ts`) to use `useToast()` directly.

5. **`web/src/pages/Logs/tabs.css` is misnamed** — it does not define `<Tabs>` markup; it contains styles for the `.segmented-control`-based List/Gallery switcher and the fullscreen button. The file and its import should be evaluated when migrating `Logs/index.tsx` to `<SegmentedControl>`.

6. **`#22c55e` and `#9ca3af` status dots** — Plan 4 notes mention replacing these with token-driven badges, but they are tiny `<span>` elements (online/offline indicators). Per the Plan 3 notes: "Status dots are *not* Badges — keep them as small CSS-only `<span>` elements with `background: var(--success)` / `background: var(--text-muted)`."

7. **`#f57c00` (warning orange) in batch.css** — There is no `--warning-strong` token. The closest is `--warning: #f59e0b`. Either use `--warning` (slightly different hue) or add `--warning-strong` to `tokens.css` if the exact value is needed.

8. **`downloads.css` danger button** — `#c0392b` / `#922b21` is a hard-coded danger red. Replace with `var(--danger)` / `var(--danger-strong)` from `tokens.css`. Both tokens already exist.

9. **`help-sidebar.css` `#fff` occurrences** — these are active/hover link colors inside the dark sidebar overlay. They should become `var(--surface)` or a literal `#fff` only inside the overlay's dark-background context. Consider whether a token makes sense, or whether `color: #fff` on a forced-dark surface is acceptable as a documented exception.

10. **`landing/src/components/Header.astro`** — the shared header uses `class="btn btn-primary"` for the login link. Changing it to `vi-btn vi-btn--primary` affects every landing page at once, which is the right move but requires verifying all pages still render correctly afterward.

## Notes from Plan 3 implementation

These are concrete lessons learned migrating `web/src/pages/Home/`. Apply them everywhere in Plan 4.

### Component APIs (verify with source before assuming)

**Button** (`shared-web/components/Button/Button.tsx`)
- Variants: `"primary" | "outline" | "ghost" | "danger" | "flat"`. No `"success"` or `"muted"`.
- No `icon` prop — the old `web/src/components/Button.tsx` wrapper had one. For icon+text buttons: put the SVG as a child with explicit `width`/`height` (e.g. `width="1.1em" height="1.1em"`) and add `style={{ gap: "0.4rem" }}` to the `<Button>`. Use `strokeWidth={1.5}` as a JSX attribute on the SVG, **not** a CSS style string — Preact may not forward style strings to SVG elements correctly.
- Does **not** default to `type="button"`. Add it explicitly on every non-submit button inside a form to prevent accidental submission.
- Anchor links styled as buttons (e.g. external download links) can't use `<Button>` — use `class="vi-btn vi-btn--primary"` / `class="vi-btn vi-btn--ghost"` directly on `<a>`.

**Badge** (`shared-web/components/Badge/Badge.tsx`)
- Variants: `"green" | "gray" | "yellow" | "red"`. No `"success"` or `"muted"` — use `"green"` and `"gray"`.
- Status dots (partner device online/offline) are **not** Badges — keep them as small CSS-only `<span>` elements with `background: var(--success)` / `background: var(--text-muted)`. The hex literals `#14b86f` and `#7e7e92` are gone from Home; apply the same token swap everywhere else.

**Card family** (`shared-web/components/Card/Card.tsx`)
- Exports: `Card`, `CardHeader`, `CardActions`, `CardGrid`. There is **no** `CardMeta` — keep `<dl class="card-meta">` as-is; the legacy `.card-meta` rule in `shared-web/components/cards.css` still applies.
- `CardActions` renders `vi-card-actions` which is `justify-content: space-between`. A single-child `CardActions` puts that child at the start — fine.
- If a card needs a compact style override, add the modifier class to `<Card class="partner-card-compact">` and update any CSS selectors from `.partner-card-compact .card-actions` → `.partner-card-compact .vi-card-actions`.

**Dialog family** (`shared-web/components/Dialog/Dialog.tsx`)
- Drop-in replacement for `web/src/components/Dialog.tsx`. Prop is `dialogRef` (not `ref`).
- `vi-dialog` applies `form { display: flex; flex-direction: column; gap: 0.85rem }` to any `<form>` inside it — this interacts with `vi-dialog-secondary-actions { margin-top: var(--space-3) }`. The combined top-gap above secondary actions is ~1.6rem, which is acceptable.
- `vi-dialog-secondary-actions` was fixed during Plan 3 to use `justify-content: flex-start` (was `center` — wrong for a "Delete device" button). **This fix is already in the source**, but confirm it still reads `flex-start` before assuming.

**Field** (`shared-web/components/Field/Field.tsx`)
- Renders `<label class="vi-field__label">` without a `for` attribute. There is no `htmlFor` prop. The label is visually associated but not programmatically — known limitation of the current API.
- Pass the `<Input>` (or any other control) as children: `<Field label="Name"><Input .../></Field>`.
- Use the `error` prop on `Field` for inline field-level validation errors; use `useToast()` for async action errors (save failed, delete failed, invite failed).

**useToast** (`shared-web/components/Toast/useToast.ts`)
- `ToastProvider` already wraps the entire app in `web/src/index.tsx` — `useToast()` works in any `/web` component without extra setup.
- Call: `const { push } = useToast(); push("message", "error" | "success" | "info")`.
- Use for async operation errors (API call failed). Keep persistent data-loading errors (SWR-level) inline.

### CSS migration recipe (concrete)

1. Replace class-based element patterns in JSX:

   | Old JSX | New JSX |
   |---|---|
   | `<div class="card">` | `<Card>` |
   | `<div class="card-header">` | `<CardHeader>` |
   | `<div class="card-actions">` | `<CardActions>` |
   | `<div class="card-grid">` | `<CardGrid>` |
   | `<span class="badge badge-green">` | `<Badge variant="green">` |
   | `<button class="btn btn-primary">` | `<Button variant="primary" type="button">` |
   | `<button class="btn btn-ghost">` | `<Button variant="ghost" type="button">` |
   | `<button class="btn btn-danger">` | `<Button variant="danger" type="button">` |
   | `<div class="field"><label>X</label><input /></div>` | `<Field label="X"><Input /></Field>` |

2. Token swaps in CSS (only exact matches; leave values without a token as-is):

   | Raw value | Token |
   |---|---|
   | `0.25rem` | `var(--space-1)` |
   | `0.5rem` | `var(--space-2)` |
   | `0.75rem` | `var(--space-3)` |
   | `1rem` | `var(--space-4)` |
   | `1.5rem` | `var(--space-5)` |
   | `2rem` | `var(--space-6)` |
   | `3rem` | `var(--space-7)` |
   | `4px` | `var(--radius-sm)` |
   | `8px` | `var(--radius-md)` |
   | `12px` | `var(--radius-lg)` |
   | `999px` | `var(--radius-full)` |
   | `0.75rem` font-size | `var(--text-xs)` |
   | `0.875rem` font-size | `var(--text-sm)` |
   | `1rem` font-size | `var(--text-base)` |
   | `1.125rem` font-size | `var(--text-lg)` |
   | `1.25rem` font-size | `var(--text-xl)` |

3. Delete dead CSS rules in each page's `style.css` — anything whose selector now refers to a class no longer present in the JSX (e.g. old `.field`, `.btn-*`, card rules that the shared-web components now own). The legacy global versions in `cards.css`, `buttons.css`, etc. are still live for un-migrated pages until 4.3, so don't touch those files yet.

4. Update any CSS selectors that reference `.card-actions` to `.vi-card-actions`, `.card-header` to `.vi-card-header`, etc., wherever those selectors live inside the page's own `style.css`.

### Build / check commands

```bash
# type check
cd web && npx tsc --noEmit

# build (also confirms prerender works)
cd web && npm run build

# hex lint — should return empty for fully-migrated directories
git grep -nE "#[0-9a-fA-F]{3,8}\b" web/src/pages/Home  # baseline: returns nothing
```

---

## Context

After Plans 1–3, the typed component library exists, the migration recipe has been validated once in Astro (landing homepage) and once in Preact (web dashboard), and two pages run on the new system in parallel with everything else still on legacy classes. Plan 4 finishes the job: migrate every remaining consumer, then delete the legacy unprefixed class definitions and the now-redundant `web/src/components/Button.tsx` / `Dialog.tsx` / `web/src/styles/alerts.css`.

End state: every hex color outside `tokens.css` is gone; every shared class is `vi-*`-prefixed; no parallel button or alert system.

## Goal

Single coherent system across both apps. Legacy classes deleted. Lint-style grep checks return empty.

## 4.1 `/web` — remaining pages

- **Auth/** — `<Field>` + `<Input>` for the login form. `<Alert variant="danger">` for the dev warning (replaces hardcoded `#c0392b` at `Auth/style.css:51-53`). `<SegmentedControl>` for the sign-in / sign-up toggle.
- **Settings/** — sections → `<Card>`. Toggles → `<Checkbox variant="switch">`, or a dedicated `<Switch>` component if Plan 1 decided to make it distinct. Replace `rgba(255, 255, 255, 0.02)` background at `Settings/style.css:48` with a token (the rgba version doesn't flip for light mode — bug fix).
- **Logs/** — biggest migration of Plan 4.
  - `<Tabs>` replaces the rolled-its-own `tabs.css`.
  - `<Card>` for list rows.
  - `<IconButton>` for toolbar buttons.
  - The legitimate dynamic inline styles in `LogsGallery.tsx:146/161/184` (virtualizer geometry) **stay inline** — they're per-row computed values.
  - The illegitimate `style="margin-left: 0.5rem"` at `LogsList.tsx:69` becomes a layout class.
  - Replace `#22c55e` and `#9ca3af` status dots in `Logs/layout.css:152,156,176,180` with token-driven badges.
  - Replace `#f57c00` and `#fff` in `Logs/batch.css` with tokens.
- **Header.tsx, ThemeButton.tsx** — `<button class="control">` → `<IconButton>`.
- **`web/src/styles/alerts.css`** → delete. Existing `.global-alert` call sites move to `useToast()` from Plan 1's `<ToastProvider>`.
- **`web/src/components/Button.tsx`, `web/src/components/Dialog.tsx`** → delete. Update imports across `/web` to `from "@virtueinitiative/shared-web"`. (Home in Plan 3 already migrated its `Dialog` import — this finishes the job.)
- **`web/src/styles/{drawer,app-shell,header,dialog}.css`** — tokenize remaining hardcoded values; delete rules now provided by components.

## 4.2 `/landing` — remaining pages

Same recipe as Plan 2, applied to:
- Blog index, blog posts (MDX) — buttons, links, code blocks, callouts.
- Downloads page — `landing/src/styles/downloads.css` has hardcoded `#c0392b` / `#922b21` for the danger button (`downloads.css:2-5`).
- Help sidebar — `landing/src/styles/help-sidebar.css` has a stack of hardcoded `#fff` and raw `rgba(...)` shadows (lines 91, 96, 146, 147, 165, 173, 174, 186).
- Tables — `landing/src/styles/tables.css` (142 lines) — replace with `<Table>` if Plan 1 added one, or tokenize in place if Plan 1 decided tables weren't worth a component yet.
- `landing/src/components/ThemeScript.astro` — keep the inlined SVGs (unavoidable for first-paint), but their sizing references the same icon-size class that `<IconButton>` uses, instead of the duplicated `1.1rem` literal.

## 4.3 Delete legacy class definitions

After every consumer above migrates:

- `shared-web/components/controls.css` → delete entirely (merged into `Button` / `IconButton`).
- `shared-web/components/buttons.css` → drop the unprefixed `.btn` rules; keep only `.vi-btn`.
- `shared-web/components/feedback.css` → drop the unprefixed `.alert-*` / `.badge-*` rules; keep only `.vi-alert` / `.vi-badge`.
- Same for `cards.css`, `segmented-control.css`, `forms.css`, `dialog.css`, `utilities.css`.
- `shared-web/index.css` — remove the `controls.css` import.

## Critical files

**Modified:**
- `web/src/pages/{Auth,Settings,Logs}/` — every `index.tsx` and `style.css`.
- `web/src/components/{Header,ThemeButton}.tsx`.
- `web/src/styles/{drawer,app-shell,header,dialog}.css`.
- Every landing page outside the homepage and every CSS file in `landing/src/styles/` outside what Plan 2 touched.
- Every `shared-web/components/*.css` (drop legacy unprefixed blocks).
- `shared-web/index.css` (remove `controls.css` import).

**Deleted:**
- `web/src/components/Button.tsx`
- `web/src/components/Dialog.tsx`
- `web/src/styles/alerts.css`
- `shared-web/components/controls.css`

## Verification

1. `pnpm -w typecheck`, `pnpm --filter web test`, `pnpm --filter web build`, `pnpm --filter landing build` — all green.
2. Full manual smoke of `/web`: Auth login flow, Home (re-verify after Plan 3 changes still hold), Settings (toggle a setting, toggle dark mode), Logs (list + gallery + batch + lightbox), header drawer on mobile, global toast from a forced error. Verify both themes throughout.
3. Full manual smoke of `/landing`: home (re-verify Plan 2), every blog post, downloads, help sidebar, mobile breakpoints. Verify both themes.
4. `git grep -nE "#[0-9a-fA-F]{3,8}\b" web/src landing/src shared-web` — every hit is either inside `shared-web/tokens.css` or inside an inline data-URI SVG. No raw hex elsewhere.
5. `git grep -nE "class=\"[^\"]*\\b(btn|control|alert-error|alert-success|global-alert|card|field|badge-green|badge-gray|badge-yellow)\\b" web/src landing/src` — empty (every legacy unprefixed name is gone from consumer markup).
6. `/dev/components` still renders cleanly — used as the visual reference throughout.
7. Bundle size sanity check: `pnpm --filter web build` and compare `dist/` size to a pre-Plan-1 baseline. Expect a small net reduction (deleted duplicate CSS) or neutral.
