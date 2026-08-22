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
      // Keep non-production deployments (staging, feature branches) out of search
      // engines by injecting a robots meta tag into the prerendered <head>.
      mode !== 'production' && {
        name: 'inject-noindex-meta',
        transformIndexHtml(html: string) {
          return html.replace(
            '</head>',
            '    <meta name="robots" content="noindex, nofollow" />\n  </head>',
          );
        },
      },
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
      // Vite's dev server answers CORS preflight requests itself (before the
      // /api proxy below even runs) using its own default cors options, which
      // don't set Access-Control-Allow-Credentials. Since api.ts's req()
      // always sets Content-Type: application/json (forcing a preflight) and
      // fetches with credentials: 'include', that default silently breaks
      // every API call in domain mode (app.<domain>.localhost, where /api is
      // proxied rather than same-port). Configure it to match what the real
      // API's cors() middleware (api/src/index.ts) sends.
      cors: {
        origin: true,
        credentials: true,
      },
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
