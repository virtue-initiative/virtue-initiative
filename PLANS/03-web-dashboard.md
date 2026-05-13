# Plan 3 — Migrate the `/web` dashboard (Home page)

> See [`00-overview.md`](./00-overview.md) for the project-wide context. Depends on [`01-foundation.md`](./01-foundation.md). Parallelizable with [`02-landing-homepage.md`](./02-landing-homepage.md).

## Context

After Plan 1 lands, the new typed Preact components and `vi-*` classes exist alongside the legacy `.btn`/`.card`/etc. No `/web` page has migrated. Plan 3 picks the most-used page in the app — the Home dashboard — and converts it. The point isn't Home itself; the point is to validate the recipe in Preact (component imports, dialog wiring, toast wiring, dark-mode behavior) before applying it across Auth/Settings/Logs in Plan 4.

## Goal

`/web/src/pages/Home/` renders entirely from typed components, with all hex colors and ad-hoc spacing replaced by tokens. Other `/web` pages stay on the legacy class layer until Plan 4. The third "online green" (`#14b86f` in `Home/style.css:153`) is gone — replaced by `var(--success)`.

## Scope

Only `web/src/pages/Home/` (`index.tsx`, `style.css`, anything imported only from there).

Out of scope: Auth, Settings, Logs, Header, Drawer, the global alert stack, and the legacy `Button.tsx` / `Dialog.tsx` wrappers in `web/src/components/`. Plan 4 handles those.

## 3.1 Component swaps

- Device list cards → `<Card>` with `<CardHeader>` / `<CardMeta>` / `<CardActions>`.
- Partner status dots (`#14b86f` / `#7e7e92`) → `<Badge variant="success">` / `<Badge variant="muted">`, or a small Home-local `<StatusDot online={…}>` helper. Decide during implementation. **The colors must come from tokens** (`--success` / `--text-muted`), eliminating the third green.
- Invite form → `<Field>` + `<Input>` + `<Checkbox>` + `<Button variant="primary">`.
- Any inline `style="..."` that isn't dynamic geometry → either a token-driven class or a prop on a component.
- Confirmation dialogs already use `Dialog` — switch the import from `web/src/components/Dialog.tsx` to `from "@virtueinitiative/shared-web"` so the old wrapper can be deleted in Plan 4.
- Any page-level success/error messages → `useToast()` (from Plan 1's `<ToastProvider>`).

## 3.2 Token swaps in `web/src/pages/Home/style.css`

Same recipe as Plan 2: hex → `var(--*)`, rem → `var(--space-*)`, radii → `var(--radius-*)`, font sizes → `var(--text-*)`, breakpoints → standard sm/md/lg queries. Delete any rules now provided by `<Card>` / `<Field>` / `<Badge>` / `<Button>` / etc.

## Critical files

- `web/src/pages/Home/index.tsx`
- `web/src/pages/Home/style.css`
- Any sub-component imported only by Home.

Untouched: `web/src/pages/{Auth,Settings,Logs}/`, `web/src/components/{Header,ThemeButton,Button,Dialog}.tsx`, `web/src/styles/*`. All of `/landing`.

## Verification

1. `pnpm --filter web dev` — Home renders correctly in light + dark, desktop + mobile (all three breakpoints).
2. Walk every interaction on Home: invite a partner, accept/reject an invite, view a partner device, open a confirmation dialog, trigger a page-local toast (e.g. force a sync error if reachable from Home). Verify both themes throughout.
3. Other `/web` pages still work end-to-end — they're on the legacy class layer, which Plan 1 deliberately preserved.
4. `pnpm --filter web test` — existing tests pass; add tests for any new Home-local logic if relevant.
5. `pnpm --filter web build` — production build succeeds.
6. `git grep -nE "#[0-9a-fA-F]{3,8}\b" web/src/pages/Home` — no hex literals remain in Home.
7. `/dev/components` still renders cleanly (sanity check).
