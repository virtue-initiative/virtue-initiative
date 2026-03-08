import { defineConfig } from "vite";

// https://vitejs.dev/config/
export default defineConfig({
  build: {
    rollupOptions: {
      input: ["index.html", "state-iframe.html"],
    },
  },
  server: {
    headers: {
      "Content-Security-Policy": `frame-ancestors http://localhost:5174 http://localhost:3000 http://localhost:5173;`,
    },
  },
});
