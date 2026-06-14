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
      allowedHosts: allowedHosts?.length ? allowedHosts : undefined,
      fs: {
        allow: [searchForWorkspaceRoot(rootDir), '..'],
      },
    },
    optimizeDeps: {
      exclude: ['@virtueinitiative/shared-web'],
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
