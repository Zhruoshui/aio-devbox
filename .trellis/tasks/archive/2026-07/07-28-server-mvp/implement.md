# Implement: AIO-style Dev Sandbox MVP

Ordered checklist. Each phase is independently verifiable - validate before moving
on. Run from repo root. See `design.md` for the architecture, §14 for the extension
model (data-driven `services.toml` + generic panes).

## Phase A - Base image + compose skeleton + gateway + app hello

- [x] `Dockerfile.base` (sandbox-base): Debian slim; install node + python + git +
      curl + ca-certs + opencode; create user `gem` uid 1000; WORKDIR /home/gem.
- [x] `app/Dockerfile`: FROM sandbox-base; build Rust axum binary; copy to /usr/local/bin.
      Minimal axum server returning "ok" on `/` for now.
- [x] `docker-compose.yml`: services `gateway` (caddy) + `app` (axum); named volume
      `workspace`; internal network; `app` mounts workspace at /home/gem as uid 1000.
- [x] `gateway/Caddyfile`: `:8080`, basicauth (env user/pass), `reverse_proxy app:8088`.
- [x] Validate: `docker compose up --build`; `curl -u user:pass http://localhost:8080/`
      -> "ok"; without auth -> 401.
- Rollback point: working gateway + app skeleton.

## Phase B - axum app: services.toml + manifest + seams + static

- [x] `app/services.toml`: declare code-server/vnc (`type=web` + target + url) and
      terminal/opencode (`type=agent` + cmd + enable env).
- [x] `config.rs`: parse `services.toml`; manifest builder iterates the list - TCP
      reachability for `web`, env flag for `agent`.
- [x] `GET /api/manifest` -> JSON service list with `enabled` per service.
- [x] `/api` `/v1` `/mcp` catch-all -> `502` seam stub JSON.
- [x] Serve a placeholder `index.html` at `/` (real SPA in Phase C).
- [x] Validate: `curl /api/manifest`; `curl /api` -> 502 seam.

## Phase C - Frontend shell (React + golden-layout, generic panes)

- [x] `web/`: Vite + React + TS; deps `golden-layout`, `@xterm/xterm`, `@xterm/addon-fit`.
- [x] `panes/IframePane.tsx` (type=web: `<iframe src={url}>`) and
      `panes/XtermPane.tsx` (type=agent: xterm.js + fit + WS `/api/term/ws?cmd=`).
- [x] `App.tsx`: fetch `/api/manifest`; map each enabled service to its generic pane
      by `type`; build golden-layout tree (placeholder content first).
- [x] Vite build -> `app/static`; app serves it at `/`.
- [x] iframe-drag transparent-overlay trick for IframePane.
- [x] Validate: `compose up`; browser -> basicauth -> workspace shell with panes.

## Phase D - code-server

- [x] `code-server/Dockerfile`: FROM sandbox-base; install code-server.
- [x] Compose service (profile `code-server`), `:8200`, mounts workspace, uid 1000.
- [x] Caddyfile: `/code-server/*` -> `code-server:8200`.
- [x] `services.toml` already declares `codeServer` (Phase B); IframePane renders it.
- [x] Validate: with `--profile code-server`, pane loads VSCode; edits persist across
      `compose restart`.

## Phase E - Terminal (pty WS + xterm)

- [x] `pty.rs`: `portable-pty` bridge; spawn shell as `gem` in `/home/gem`; pipe to WS.
- [x] `GET /api/term/ws` (WS upgrade); optional `?cmd=`.
- [x] `services.toml` declares `terminal` (Phase B); XtermPane (Phase C) renders it.
- [x] Validate: terminal pane runs `ls`, `echo`; files written persist in /home/gem.

## Phase F - opencode

- [x] Ensure `opencode` is installed in sandbox-base (Phase A) / app image.
- [x] `services.toml` declares `opencode` (`type=agent`, `cmd=opencode`); XtermPane
      opens `/api/term/ws?cmd=opencode`.
- [x] Validate: opencode launches in its pane.

## Phase G - VNC / Chromium   (verified 2026-07-28, non-visual)

- [x] `vnc/Dockerfile`: FROM sandbox-base; install TigerVNC, websockify + noVNC,
      openbox, Chromium; bash supervisor (`vnc/entrypoint.sh`) instead of
      s6-overlay (design §7 / implement.md risky-note allows the fallback).
- [x] Compose service (profile `vnc`), `:6080`, mounts workspace, uid 1000,
      `shm_size: 2gb`.
- [x] Caddyfile: `/vnc/*` -> `vnc:6080` (`handle_path` strip-prefix).
- [x] `services.toml` declares `vnc`; IframePane renders `/vnc/vnc.html?...`.
- [x] Non-visual validation: 4 procs run as `gem`; `vnc.html` HTTP 200; WS
      upgrade through basicauth = 101, without auth = 401; manifest flips
      `vnc.enabled` on container stop/start (AC4 mechanism); Chromium profile
      on the shared volume survives restart + Chromium relaunches (AC3); X
      framebuffer screenshot is ~80% non-black (content rendering).
- [x] Visual validation (user-confirmed 2026-07-28): with `--profile vnc`, the
      noVNC pane connects and drives Chromium - user navigated to
      www.baidu.com (AC2 "VNC drives Chromium"). Two real bugs found+fixed en
      route: noVNC WS `path=vnc/websockify` (services.toml) and Chromium
      SingletonLock cleanup (entrypoint.sh) - see "Risky points". Also added
      `fonts-noto-cjk` to vnc/Dockerfile so CJK pages render server-side.

## Phase H - Pluggability polish + acceptance

- [x] Verify manifest reflects profiles (run with/without `--profile vnc`).
      VERIFIED 2026-07-28 (non-visual): `docker compose --profile vnc stop vnc` ->
      `vnc.enabled=false` (codeServer/terminal/opencode stay true); `start vnc` ->
      `vnc.enabled=true`. AC4 mechanism at the manifest level.
- [x] Verify UI hides absent panes.  (Code-level confirmed: App.tsx filters
      `s.enabled`; manifest flips per the item above. VISUAL pending user: open the
      UI, stop vnc, confirm no VNC pane renders.)
- [x] Extension smoke test: add a throwaway `type=agent` service to `services.toml`
      -> pane appears with no React change (proves §14A). VERIFIED 2026-07-28:
      added `smokeTest` (type=agent, cmd=`echo ...`), rebuilt app, manifest
      returned 5 services with `smokeTest` enabled; the frontend is generic
      (`PaneForService` dispatches on `type` only, `buildLayoutConfig` makes one
      pane per enabled service). Reverted `services.toml` + Dockerfile; manifest
      back to 4 services. (See "Risky points" for the offline-build workaround.)
- [x] Run AC1-AC5 (`prd.md`) - non-visual parts VERIFIED 2026-07-28:
      - AC1: `GET /` -> SPA `index.html` + `/assets/index-*.js|.css` (200); `401`
        without auth. (VISUAL pending: browser loads 4 arrangeable panes.)
      - AC2: code-server serves the IDE at `/code-server/?folder=/home/gem` (302->200,
        asset refs present); opencode v1.18.7 installed in the app container; terminal
        pty WS (`/api/term/ws`) bidirectional - spawns shell as `gem` in `/home/gem`,
        `echo <marker>` input returns in the output stream. (VISUAL pending: code-server
        edits, VNC drives Chromium + CJK renders, opencode pane launch.)
      - AC3: shared `workspace` volume visible to app + code-server + vnc (same marker
        file); marker survives an app container `restart`.
      - AC4: manifest flips `vnc.enabled` on stop/start (item 1). (VISUAL pending:
        no VNC pane when profile absent.)
      - AC5: `/api`, `/api/`, `/api/*`, `POST /api`, `/v1`, `/v1/*`, `/mcp`, `/mcp/*`
        -> 502 `{"error":"seam reserved"}`; `/api/manifest` (200) and `/api/term/ws`
        (WS handler, not the seam) are NOT swallowed.
- [x] Final full-scope check (all packages): `npm run build` (web) = `tsc --noEmit`
      clean + `vite build` OK (74 modules; output asset hashes match the running
      image). App has no unit tests; compile proof = 2 successful image rebuilds
      during the smoke test. (Chunk-size warning >500kB is a non-blocking advisory
      from golden-layout + xterm.)
- VISUAL ACCEPTANCE CONFIRMED 2026-07-29 (user): AC1 (4 panes load,
  arrangeable), AC2 (code-server edits, VNC drives Chromium + CJK renders,
  terminal runs commands, opencode launches + fills width via the pty-resize
  fix). AC3/AC4/AC5 were already non-visual-verified. All AC1-AC5 pass.

## Finish status

- **Committed**: `a83508b` on branch `feat/aio-sandbox-mvp` (40 files, root
  commit). Includes the 4 integration fixes (noVNC path, SingletonLock, CJK
  font, pty resize) and `vnc/DEFERRED-chromium-decorations.md`.
- **Chromium window-decoration polish**: explored (openbox decor / tint2
  taskbar / session-restore / CSD-disable flag) but **reverted** to the first
  working version - Chromium 150's CSD buttons can't be hidden on X11
  (upstream limit). Full investigation + re-apply notes in
  `vnc/DEFERRED-chromium-decorations.md`; deferred for later.
- **3.3 spec update**: DEFERRED to the `00-bootstrap-guidelines` task (fills
  `.trellis/spec/` with the project's real conventions). Task left
  `in_progress` until that lands; archive after.

## Validation commands

- `docker compose --profile code-server --profile vnc up --build`
- `docker compose restart` -> verify persistence (AC3)
- `docker compose up` (no profiles) -> only terminal + opencode panes (AC4)
- `curl -u user:pass http://localhost:8080/api/manifest`
- `cargo test` (app), `npm run build` (web)

## Risky points / rollback

- **code-server behind `/code-server/` subpath**: if it misbehaves, fall back to a
  dedicated caddy route or code-server `--prefix`. Test early (Phase D).
- **noVNC WS through caddy basicauth**: RESOLVED in Phase G - WS upgrade returns
  101 with auth, 401 without (basicauth covers the WS upgrade).
- **noVNC WS path behind `/vnc/` subpath**: RESOLVED in Phase G. noVNC `ui.js`
  builds the WS URL as `ws://<host>:<port>/<path>` (absolute - it does NOT keep
  the `/vnc/` subpath the page was served from). Default `path=websockify` ->
  `ws://host/websockify` -> caddy catch-all -> axum -> 404. Fix: iframe URL
  carries `&path=vnc/websockify` so it connects to `/vnc/websockify`, which
  caddy strips to `/websockify` on vnc:6080. host/port auto-derive from
  window.location, so only `path` needs setting. (services.toml `vnc.url`.)
- **Chromium SingletonLock across container recreate**: RESOLVED in Phase G.
  Chromium writes `Singleton{Lock,Cookie,Socket}` symlinks (target
  `<hostname>-<pid>`) in the profile dir on the shared volume. A container
  RECREATE changes the hostname, so the next start sees a foreign-hostname lock,
  refuses to launch ("profile appears to be in use by another computer"), exits,
  and the bash supervisor tears the container down -> crash loop. A plain
  `docker restart` (same hostname) does NOT trigger it, so it eluded early AC3
  testing. Fix: `vnc/entrypoint.sh` removes stale `Singleton*` before launching
  chromium (safe - only one chromium runs per container). This is what makes the
  persisted profile survive recreates (AC3).
- **portable-pty as uid 1000**: ensure the app process runs as `gem`; test in Phase E.
- **VNC container multi-process**: s6 config is fiddly; allow a supervisord fallback.
- **docker.io unreachable in this sandbox (build env, not code)**: `registry-1.docker.io`
  is blocked by the sandbox network policy, so BuildKit cannot pull the
  `# syntax=docker/dockerfile:1` frontend (app/Dockerfile line 1) - `docker compose
  build app` fails at "resolve image config for docker/dockerfile:1" with EOF. The
  Dockerfile uses only standard multi-stage `COPY --from`, so it does not need that
  frontend. Workaround used for the Phase H smoke-test rebuilds: temporarily replace
  line 1 with a plain comment so BuildKit falls back to its bundled default frontend
  (all base images - `rust:1-bookworm`, `sandbox-base`, `node:20-bookworm` - are
  already cached, so the build is fully offline). Restored line 1 afterward; the
  running binary is unaffected. On a host with docker.io reachable this is a no-op.
