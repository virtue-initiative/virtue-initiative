import { defineConfig, loadEnv, searchForWorkspaceRoot } from "vite";
import preact from "@preact/preset-vite";

// https://vitejs.dev/config/
export default defineConfig(({ mode }) => {
  const rootDir = new URL(".", import.meta.url).pathname;
  const env = loadEnv(mode, rootDir, "");
  const allowedHosts = env.__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS
    ?.split(",")
    .map((host) => host.trim())
    .filter(Boolean);

  return {
    plugins: [
      preact({
        prerender: {
          enabled: true,
          renderTarget: "#app",
          additionalPrerenderRoutes: ["/404"],
          previewMiddlewareEnabled: true,
          previewMiddlewareFallback: "/404",
        },
      }),
    ],
    server: {
      allowedHosts: allowedHosts?.length ? allowedHosts : undefined,
      fs: {
        allow: [searchForWorkspaceRoot(rootDir), ".."],
      },
    },
    optimizeDeps: {
      exclude: ["@virtueinitiative/shared-web"],
    },
    resolve: {
      preserveSymlinks: false,
    },
  };
});
