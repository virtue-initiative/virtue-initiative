# shared-web

UI components, design tokens and API types shared by `web/` and `landing/`. It is a Bun
workspace package (`@virtueinitiative/shared-web`), imported as source, so there is no build
step.

- `components/`: Preact components (Button, Dialog, Field, Toast, ...), exported from
  `components/index.ts`.
- `tokens.css`, `base.css`, `utilities.css`: design tokens and base styles. Read
  [`DESIGN-GUIDELINES.md`](DESIGN-GUIDELINES.md) before changing them or styling a component.
- `types.ts`: Zod schemas for API responses, the single source for the shapes the API returns
  and the web app reads (see `api/AGENTS.md`).
- `risk.ts`: the concern-level thresholds (WEB-024), shared with the client core's event counts.
- `api-version.ts`: the API version prefix, written by `client/scripts/update-version.sh`.

## Checks

This package has no tests or typecheck of its own. After changing it, run `bun run format:check`
here, then the typecheck and build of both `web/` and `landing/`, which is where breakage shows
up.
