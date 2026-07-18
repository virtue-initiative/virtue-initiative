import { defineConfig } from 'astro/config';

import mdx from '@astrojs/mdx';

import preact from '@astrojs/preact';

import mermaid from 'astro-mermaid';

export default defineConfig({
  trailingSlash: 'never',
  integrations: [
    mermaid({
      theme: 'base',
      autoTheme: false,
      // `themeVariables` must live under `mermaidConfig` — astro-mermaid's top-level
      // options object only reads theme/autoTheme/mermaidConfig/iconPacks/enableLog
      // and silently drops anything else, including a top-level `themeVariables`.
      mermaidConfig: {
        themeVariables: {
          primaryColor: '#ebe4ce', // --bg-subtle
          primaryTextColor: '#1b1a16', // --text
          primaryBorderColor: '#1e3a2e', // --accent
          lineColor: '#6a6655', // --text-muted
          background: '#fbf7ea', // --surface
          textColor: '#1b1a16',
          // Deliberately a system stack, not the site's "IBM Plex Sans" webfont: Mermaid
          // measures node/text box sizes with canvas measureText at render time, which
          // runs before the async Google Fonts request resolves. If the configured font
          // isn't loaded yet, boxes get sized against fallback-font metrics and then
          // overflow once the real font swaps in. System fonts are always available
          // immediately, so there's no swap and no size mismatch.
          fontFamily: 'ui-sans-serif, system-ui, sans-serif',
        },
        // Mermaid's default HTML-label rendering sizes each node via a `<foreignObject>`
        // measured in a hidden div; in Firefox in particular that measured width can
        // disagree with the box dagre actually lays out, clipping label text against
        // the node edge. Plain SVG `<text>`/`<tspan>` sizing doesn't have this bug.
        // Must be the top-level `htmlLabels`, not `flowchart.htmlLabels` — that's a
        // deprecated per-diagram key that most mermaid v11 render paths ignore in
        // favor of this global one.
        htmlLabels: false,
      },
    }),
    mdx(),
    preact({ compat: true }),
  ],
  vite: {
    esbuild: {
      jsx: 'automatic',
      jsxImportSource: 'preact',
    },
    optimizeDeps: {
      esbuildOptions: {
        jsx: 'automatic',
        jsxImportSource: 'preact',
      },
    },
  },
});
