# Design Guidelines — Warm Institutional

The Virtue Initiative UI uses a warm, institutional aesthetic: cream paper
backgrounds, a deep forest-green primary, serif headings paired with a humanist
sans body and a mono for labels, and near-square corners. This document is the
reference for the tokens defined in `tokens.css`. Always reference tokens — never
hardcode hex values in component CSS.

## Palette

### Paper / backgrounds

| Token                       | Hex       | Role                                             |
| --------------------------- | --------- | ------------------------------------------------ |
| `--bg` / `--paper`          | `#F4EFE3` | App background (warm cream)                      |
| `--surface` / `--paper-3`   | `#FBF7EA` | Cards, dialogs, raised surfaces (lightest paper) |
| `--bg-subtle` / `--paper-2` | `#EBE4CE` | Inset/recessed fills, hover wells (warmer card)  |

### Rules / borders

| Token                    | Hex       | Role                                             |
| ------------------------ | --------- | ------------------------------------------------ |
| `--border`               | `#D9D1BC` | Default hairline rule on cards, inputs, dividers |
| `--rule-2`               | `#E8E1CB` | Lighter secondary rule                           |
| `--control-border-hover` | `#C9C0A8` | Border on control hover                          |

### Ink / text

| Token                      | Hex       | Role                           |
| -------------------------- | --------- | ------------------------------ |
| `--text` / `--ink`         | `#1B1A16` | Primary text (warm near-black) |
| `--ink-2`                  | `#3A382F` | Secondary text                 |
| `--text-muted` / `--ink-3` | `#6A6655` | Muted text, captions, hints    |
| `--ink-4`                  | `#9C9682` | Faint text, disabled hints     |

### Forest (primary)

| Token                   | Hex       | Role                                      |
| ----------------------- | --------- | ----------------------------------------- |
| `--accent` / `--forest` | `#1E3A2E` | Primary actions, links, active indicators |
| `--forest-2`            | `#163026` | Deep forest (pressed/emphasis)            |
| `--forest-3`            | `#2C4D3E` | Forest hover                              |

### Moss (secondary green)

| Token      | Hex       | Role                                    |
| ---------- | --------- | --------------------------------------- |
| `--moss`   | `#4D6B58` | Secondary green, active control borders |
| `--moss-2` | `#7A8F76` | Lighter moss accents                    |

### Ochre (accent — use sparingly)

| Token          | Hex       | Role                                 |
| -------------- | --------- | ------------------------------------ |
| `--ochre`      | `#A67F3D` | Sparing accent / highlight           |
| `--ochre-soft` | `#C9A56B` | Softer ochre, dark-mode warning text |

### Status

| Token                  | Hex       | Role                |
| ---------------------- | --------- | ------------------- |
| `--success` / `--good` | `#4F7A5A` | Success / healthy   |
| `--warning`            | `#9C6B2E` | Warning / caution   |
| `--danger`             | `#ef4444` | Error / destructive |
| `--danger-strong`      | `#dc2626` | Stronger error      |

Status fills on badges/alerts use a low-opacity tint of the status color
(`rgba(... , 0.12)`) with the solid color as text.

## Typography

Three families, loaded via Google Fonts in `base.css`:

- **Serif** — `var(--font-serif)` → `"Source Serif 4", Georgia, serif`.
  Use for headings and editorial display text.
- **Body** — `var(--font)` → `"IBM Plex Sans", ui-sans-serif, sans-serif`.
  Default for all body copy, buttons, inputs, controls.
- **Mono** — `var(--font-mono)` → `"IBM Plex Mono", ui-monospace, monospace`.
  Use for eyebrow labels, metadata, codes, and other "stamped" small-caps-feeling
  labels.

Rule of thumb: serif for what you read, sans for what you operate, mono for what
you reference.

## Shape (corner radius)

Near-square. Corners are tightened relative to the previous design:

| Token           | Value   | Use                              |
| --------------- | ------- | -------------------------------- |
| `--radius-sm`   | `2px`   | Pills/badges, buttons, inputs    |
| `--radius-md`   | `4px`   | Cards, dialogs, larger surfaces  |
| `--radius-lg`   | `6px`   | Reserved; avoid on components    |
| `--radius-xl`   | `8px`   | Reserved; avoid on components    |
| `--radius-full` | `999px` | Avatars / circular elements only |

Badges use a literal `2px` (square pill), not `--radius-full`.

## Spacing

Use the `--space-1` … `--space-8` scale (`0.25rem` → `4rem`). Prefer tokens over
ad-hoc rem values for gaps and margins.

## Component variant intent

- **Button**
  - `--primary`: filled forest, paper text; hover → `--forest-3`.
  - `--outline`: forest text + border on transparent; hover fills forest.
  - `--ghost`: muted text with a hairline border; subtle hover well.
  - `--quiet`: subtle paper fill with border — low-emphasis neutral action.
  - `--danger`: danger outline that fills lightly on hover.
  - `--flat`: text-only, no border.
- **Card** — surface paper on a hairline border, `--radius-md`. `--highlight`
  swaps the border to forest. `--tight` / `--flush` adjust padding.
- **Badge** — square (2px) pills. Tinted status fills with solid status text.
- **Input** — cream `--bg` fill, hairline border, `--radius-sm`; focus border
  goes forest.
- **Tabs / SegmentedControl** — active state reads forest: tabs underline in
  forest; segmented control fills the active item forest with paper text.
- **Toast** — paper surface with a hairline border and a 3px status-colored
  left rule.
