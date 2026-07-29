import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The SPA is served at "/" by the axum app's ServeDir behind the caddy gateway.
// `base: "/"` keeps asset URLs root-absolute. `build.outDir: "dist"` is copied
// into the image at /app/static by app/Dockerfile's web-builder stage.
//
// Dev mode (`npm run dev`) is best-effort: the dev server proxies /api,
// /code-server and /vnc to the running stack on :8080, but that endpoint sits
// behind caddy basicauth, so unauthenticated proxied requests will 401. Dev is
// not the primary validation path; production validation is the Docker build.
export default defineConfig({
  plugins: [react()],
  base: "/",
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
  server: {
    proxy: {
      "/api": "http://localhost:8080",
      "/code-server": { target: "http://localhost:8080", ws: true },
      "/vnc": { target: "http://localhost:8080", ws: true },
    },
  },
});
