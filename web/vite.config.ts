/// <reference types="vitest" />
import { defineConfig, loadEnv, searchForWorkspaceRoot } from 'vite';
import preact from '@preact/preset-vite';

// https://vitejs.dev/config/
export default defineConfig(({ mode }) => {
  const rootDir = new URL('.', import.meta.url).pathname;
  const env = loadEnv(mode, rootDir, '');
  const allowedHosts = env.__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS
    ?.split(',')
    .map((host) => host.trim())
    .filter(Boolean);

  return {
    define:
      mode === 'test'
        ? {
            'import.meta.env.VITE_API_URL': JSON.stringify('http://localhost:8787'),
          }
        : undefined,
    plugins: [
      preact({
        prerender: {
          enabled: true,
          renderTarget: '#app',
          additionalPrerenderRoutes: ['/404'],
          previewMiddlewareEnabled: true,
          previewMiddlewareFallback: '/',
        },
      }),
    ],
    server: {
      proxy: {
        '/api': {
          target: process.env.VITE_API_PROXY_TARGET ?? 'http://localhost:8787',
          changeOrigin: true,
        },
        '/r2': {
          target: process.env.VITE_API_PROXY_TARGET ?? 'http://localhost:8787',
          changeOrigin: true,
        },
      },
      allowedHosts: allowedHosts?.length ? allowedHosts : undefined,
      fs: {
        allow: [searchForWorkspaceRoot(rootDir), '..'],
      },
      // OPFS synchronous VFS requires cross-origin isolation
      headers: {
        'Cross-Origin-Opener-Policy': 'same-origin',
        'Cross-Origin-Embedder-Policy': 'require-corp',
      },
    },
    // The cache worker imports shared chunks, so it needs code-splitting, which
    // is only supported with the ES module worker format (default is 'iife').
    worker: {
      format: 'es',
    },
    optimizeDeps: {
      exclude: ['@virtueinitiative/shared-web', '@sqlite.org/sqlite-wasm'],
    },
    resolve: {
      preserveSymlinks: false,
      dedupe: ['preact'],
    },
    test: {
      environment: 'happy-dom',
      globals: true,
      setupFiles: ['./src/test-setup.ts'],
      silent: true,
      server: {
        deps: {
          // Inline so Vite applies the react→preact/compat alias (avoids duplicate hook system)
          inline: ['@tanstack/react-virtual', '@tanstack/virtual-core'],
        },
      },
    },
  };
});
