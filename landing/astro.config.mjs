import { defineConfig } from 'astro/config';

import mdx from '@astrojs/mdx';

import preact from '@astrojs/preact';

import sitemap from '@astrojs/sitemap';

import mermaid from 'astro-mermaid';

// Pages that exist but shouldn't be offered to search engines: the 404 page
// and the post-checkout thank-you page.
const EXCLUDED_FROM_SITEMAP = new Set(['/404', '/donate/success']);

export default defineConfig({
  site: 'https://virtueinitiative.org',
  trailingSlash: 'never',
  build: {
    format: 'file',
  },
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
    // Writes sitemap-index.xml + sitemap-0.xml against `site`, which
    // src/pages/robots.txt.ts advertises to crawlers.
    sitemap({
      filter: (page) => !EXCLUDED_FROM_SITEMAP.has(new URL(page).pathname),
    }),
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
