import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The SPA is served at "/" by aio-mgr's ServeDir (mgr/src/main.rs), reached
// either directly on :8089 (bare-metal form) or through the total gateway's
// mgr.localhost site (containerized form). `base: "/"` keeps asset URLs
// root-absolute; `build.outDir: "dist"` is what mgr/Dockerfile's web-builder
// stage copies into the image at /app/static.
//
// Dev mode (`npm run dev`): the dev server proxies /api to a bare-metal
// aio-mgr on :8089. Unlike the workbench SPA there is no auth in front of
// mgr (prd D9), so the proxy is the full dev story - no 401 caveat. The
// workspace's terminal pane opens a WebSocket on /api/sbx/<name>/api/term/ws,
// so the proxy must also forward upgrades (same as the workbench's
// /code-server and /vnc entries in web/vite.config.ts).
export default defineConfig({
  plugins: [react()],
  base: "/",
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
  server: {
    proxy: {
      "/api": {
        target: "http://localhost:8089",
        ws: true,
      },
    },
  },
});
