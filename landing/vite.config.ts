import { defineConfig } from "vite";

// https://vitejs.dev/config/
export default defineConfig({
  server: {
    headers: {
      'Content-Security-Policy': `frame-ancestors http://localhost:5174 http://localhost:3000 http://localhost:5173;`
    }
  }
});

