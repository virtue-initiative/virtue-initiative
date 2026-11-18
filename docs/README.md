# Documentation map

Where each kind of doc lives. When a change makes a doc wrong, fix it in the same change.
`just doc-check` (run in CI) fails when a doc names a file that doesn't exist.

## By kind

| Kind                 | What it's for                                                | Where                                                                           |
| -------------------- | ------------------------------------------------------------ | ------------------------------------------------------------------------------- |
| Overview             | What the system is for and how the parts fit                 | [`overview.md`](overview.md)                                                    |
| Specs                | Exact required behavior. The source of truth, cited by tests | `SPEC.md` in each component (table below)                                       |
| How to build and run | Getting a component running on a dev machine                 | Each component's `README.md`, and [`development.md`](development.md)            |
| Design               | Why something works the way it does                          | `client/core/architecture.md`, `client/*/DESIGN.md`, [`decisions/`](decisions/) |
| Releasing            | Signing, stores and release pipelines                        | `client/*/RELEASING.md`, `api/README.md`, `api-donate/README.md`                |
| Agent instructions   | Rules that are easy to break, for coding agents              | `AGENTS.md` at the root and in `api/`, `web/`, `client/`, `client/core/`        |
| Testing              | How specs and tests connect                                  | [`../TESTING.md`](../TESTING.md), `api/TESTING.md`                              |
| CI                   | The exact commands each workflow runs                        | [`ci-checks.md`](ci-checks.md)                                                  |
| User help            | What users read on the website                               | `landing/src/content/help/`, `landing/src/content/download-instructions/`       |

Specs say _what_ happens, in RFC language, and change before the code does. Everything else
says _how_ to work with it or _why_ it is that way, and should link to spec IDs rather than
repeat them.

## Specs

| Prefix   | File                                              | Covers                                   |
| -------- | ------------------------------------------------- | ---------------------------------------- |
| `API`    | [`api/SPEC.md`](../api/SPEC.md)                   | The main API                             |
| `DONATE` | [`api-donate/SPEC.md`](../api-donate/SPEC.md)     | The donations Worker                     |
| `WEB`    | [`web/SPEC.md`](../web/SPEC.md)                   | What users see and do in the web app     |
| `HASH`   | [`hash-server/SPEC.md`](../hash-server/SPEC.md)   | The hash server                          |
| `CORE`   | [`client/core/SPEC.md`](../client/core/SPEC.md)   | The shared Rust client core              |
| `CLIENT` | [`client/SPEC.md`](../client/SPEC.md)             | What users see and do in the client apps |
| `BATCH`  | [`batch-format/SPEC.md`](../batch-format/SPEC.md) | The encrypted batch wire format          |

## By component

| Component      | Start here                                                                                                                                                              |
| -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `api/`         | [README](../api/README.md), [AGENTS](../api/AGENTS.md), [TESTING](../api/TESTING.md)                                                                                    |
| `api-donate/`  | [README](../api-donate/README.md)                                                                                                                                       |
| `web/`         | [README](../web/README.md), [AGENTS](../web/AGENTS.md)                                                                                                                  |
| `landing/`     | [README](../landing/README.md)                                                                                                                                          |
| `shared-web/`  | [README](../shared-web/README.md), [design guidelines](../shared-web/DESIGN-GUIDELINES.md)                                                                              |
| `hash-server/` | [README](../hash-server/README.md)                                                                                                                                      |
| `client/`      | [README](../client/README.md), [AGENTS](../client/AGENTS.md); then each platform's README                                                                               |
| `client/core/` | [architecture](../client/core/architecture.md), [AGENTS](../client/core/AGENTS.md), [tampering](../client/core/tampering.md), [models](../client/core/models/README.md) |

## Other

- [`unused-code.md`](unused-code.md): finding unused TypeScript and CSS.
- [`runbooks/`](runbooks/): one-off operational procedures.
- [`../scripts/nsfw-eval/README.md`](../scripts/nsfw-eval/README.md): evaluating the NSFW classifier.
- [`../images/README.md`](../images/README.md): the repo's image assets.
