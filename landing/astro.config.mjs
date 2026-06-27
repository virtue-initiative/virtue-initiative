import { defineConfig } from 'astro/config';

import mdx from '@astrojs/mdx';

import preact from '@astrojs/preact';

export default defineConfig({
  trailingSlash: 'never',
  integrations: [mdx(), preact({ compat: true })],
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
