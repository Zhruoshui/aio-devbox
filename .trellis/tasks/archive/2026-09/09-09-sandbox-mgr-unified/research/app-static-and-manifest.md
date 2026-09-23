# Research: app static serving, manifest build, buttons/preview APIs, redirect-page replacement, PI_WEB_URL/MGR_URL env handling

- **Query**: app/src/main.rs static serving + routes; config.rs build_manifest probe logic; buttons.rs + preview.rs API shapes; redirect page replacement surface (Dockerfile bake, entrypoint); PI_WEB_URL / MGR_URL env handling.
- **Scope**: internal
- **Date**: 2026-09-09

## 1. app/src/main.rs — static serving + full route table

- Static dir: `STATIC_DIR = "/app/static"` (main.rs:58) — must stay OUTSIDE /root (workspace volume mount). `ServeDir::new(STATIC_DIR).fallback(ServeFile::new(STATIC_DIR/index.html))` (main.rs:117-118); SPA fallback for hard loads.
- Startup config resolution (main.rs:73-103):
  - `AIO_BUTTONS_FILE` default `/root/.aio/buttons.toml` (:63, :73-75).
  - `AIO_MODELS_FILE` default `/root/.aio/models.json` (:67, :76-77).
  - services loaded from baked services.toml, urls `{env:VAR:default}`-expanded ONCE; piWeb special-cased: PI_WEB_URL override REPLACES verbatim, skipping expansion (main.rs:84-97).
  - `mgr_url = config::mgr_url()` (:102) → AppState (:103); `spawn_mgr_sync` when set (:111-113).
  - `spawn_stats_sampler` (:106).
- Route table (main.rs:120-186), in registration order:
  - `GET /api/manifest` (:122)
  - `GET /api/stats` (:126)
  - `GET /api/term/ws` (:131)
  - `POST /api/buttons` (:135), `GET /api/buttons/probe` (:136), `DELETE /api/buttons/:id` (:137)
  - `GET|PUT /api/models/config` (:141)
  - `POST /api/models/import/pi` (:142)
  - `POST /api/models/discover` (:143), `POST /api/models/test` (:144)
  - `GET /api/models/agents` (:145), `POST /api/models/apply/:agent` (:146)
  - `PUT|DELETE /api/models/agents/:agent/provider/:id` (:150-153), `POST /api/models/agents/:agent/sync` (:154)
  - `GET /api/models/usage` (:156)
  - `GET /api/models/managed` (:160)
  - `GET /api/models/catalog` (:162)
  - `/preview/:port`, `/preview/:port/`, `/preview/:port/*path` any-method (:169-171)
  - seam 502: `/api`, `/api/`, `/api/*rest`, `/v1` trio, `/mcp` trio (:176-184)
  - `.fallback_service(serve_dir)` (:185)
- Binds `0.0.0.0:8088` (:188-190).

**D1 (web/ deprecated, app keeps backend APIs + redirect page)**: the replacement only touches the SPA tree + fallback. Everything under `/api`, `/preview`, seams stays. The `ServeDir` at `/app/static` would serve a new minimal redirect page (or the dir content is swapped in the Dockerfile web-builder stage); the `index.html` fallback keeps deep-link hard loads working by redirecting.

## 2. app/src/config.rs — manifest build

- Two button sources merged at manifest time (module comment :1-17):
  1. `services.toml` baked via `include_str!` (:25, load_services :208-212 — startup panic on invalid).
  2. `/root/.aio/buttons.toml` runtime on the workspace volume (load_buttons :316-347; parse_button_defs :288-305 — missing/empty/malformed = empty + warn, never breaks the manifest).
- merge_services: built-in wins on id collision (:351-364).
- `build_manifest(services, dirs)` (:367-391) — `enabled` computed live per request:
  - `ServiceType::Web` → `is_web_reachable(svc)` (:408-417): TCP connect to `svc.target` with 400ms timeout; any error = not enabled.
  - `ServiceType::Agent` → `command_exists(cmd, dirs)` (:181-202): first whitespace token probed against cached login-shell PATH dirs (`resolve_path_dirs` :144-160, TTL 60s PATH_CACHE_TTL :31; resolved via `bash -lc 'printf %s "$PATH"'` :163-174). Empty cmd = true (terminal).
  - `ServiceType::Page` → always true (:95-96 — page panes are app-native).
- ManifestEntry serialization (:99-115): `type` renamed, `url`/`cmd` omitted when None (`skip_serializing_if`).
- Placeholder expansion `expand_placeholders` (:223-240) + `expand_one` (:270-283): `{env:VAR:default}`; set non-empty VAR → value; unset/EMPTY → default; malformed → verbatim. Runs once at startup (main.rs), not per request.
- humanize_id fallback labels (:395-403): codeServer→"code-server", vnc→"Chromium", terminal→"Terminal", modelsConfig→"Model config".

### services.toml (app/services.toml) — the built-in entries

| id | type | target | url | label | line |
|---|---|---|---|---|---|
| codeServer | web | `app:8200` | `/code-server/` | code-server | :27-32 |
| vnc | web | `app:6080` | `/vnc/vnc.html?autoconnect=1&resize=scale&path=vnc/websockify` | Chromium | :34-47 |
| terminal | agent | — | cmd `""` | Terminal | :49-53 |
| opencode | agent | — | cmd `opencode` | opencode | :55-59 |
| pi | agent | — | cmd `pi` | pi | :61-65 |
| piWeb | web | `app:30141` | `http://{host}:{env:PI_WEB_HOST_PORT:30141}/` | pi Web | :67-103 |
| modelsConfig | page | — | — | 模型配置 | :105-108 |

- vnc url `path=vnc/websockify` is REQUIRED (noVNC builds absolute WS URL — :39-46).
- piWeb published-port rationale (:68-100): Next.js root-absolute assets break under subpath proxy; `{host}` substituted client-side by IframePane.

## 3. buttons.rs — API shapes

- `POST /api/buttons` body `ButtonInput { label, cmd (default ""), type (rename of button_type, default ""), port: Option<u16> }` (buttons.rs:31-39).
- Response 201 `ButtonOut { id, label, type, cmd, port? }` (:42-51).
- Validation `validate_shape` (:99-127): agent requires non-empty cmd ≤64 chars; web requires port 1-65535, port 0/8088 rejected (8088 = axum itself, recursion); unknown type 400. Web rows normalize cmd to "".
- id: slugify(label) + `-2`/`-3` dedup (:186-213); writes atomic (temp+rename, mkdir -p) under `state.file_lock` (:217-229).
- `GET /api/buttons/probe?port=N` (:147-166): TCP probe `127.0.0.1:port` 400ms; port 0/8088/non-numeric → 400; returns `{listening: bool}`.
- `DELETE /api/buttons/:id` (:168-181): 404 when absent; 204 on success.

## 4. preview.rs — /preview/:port proxy

- Port guard: 0 and 8088 rejected (preview.rs:38-40); non-numeric → 404 (:87-95).
- Path strip: `/preview/<port>` and `/preview/<port>/` → `/` (:46-54).
- HTTP forwarding (:120-161): hop-by-hop headers stripped (:59-65), Host dropped (hyper sets from URL), body streamed via `Body::from_stream(resp.bytes_stream())`.
- WS tunneling (:167-272): detect upgrade (:104-113); tokio-tungstenite plaintext connect with 2s timeout (:72, :185-200); negotiated subprotocol echoed back (:207-218); bidirectional message pump (:220-271); Raw frames dropped.
- Known boundary: root-absolute asset URLs (vite default) break under ANY subpath proxy — upstream must configure base (:19-22).

**D7 (buttons stay in sandbox buttons.toml managed via mgr proxy)**: mgr-web's register-dialog + delete + probe calls must be routed through the per-sandbox proxy (the app's own /api/buttons*), i.e. proxy path forms like `/api/sbx/<name>/...` forwarding to `http://sbx-<name>-piweb:8088/api/buttons...`. The buttons.rs semantics (id dedup, slug, atomic write, probe port rules incl. 8088) need no change — they live sandbox-side.

## 5. Redirect page replacement surface (D1)

Where the SPA is baked & referenced:
- `app/Dockerfile:69-77` — web-builder stage: `FROM ${BASE_IMAGE} AS web-builder`, `WORKDIR /web`, `COPY web/package.json web/package-lock.json`, `npm ci`, `COPY web/ ./`, `RUN npm run build` → `/web/dist`.
- `app/Dockerfile:92-94` — `COPY --from=web-builder /web/dist /app/static` (comment: under /app, not /root).
- `app/Dockerfile:98-99` — ENTRYPOINT `/usr/local/bin/aio-entrypoint.sh`, CMD `aio-app`.
- `app/entrypoint.sh:37-47` — pi-web autostart respawn loop (port 30141, log `~/.aio/pi-web.log`), `PI_WEB_ALLOWED_HOSTS` default "app"; unconditional `exec "$@"`.
- `app/src/main.rs:117-118` — ServeDir + index.html fallback (the only consumer of /app/static).
- Runtime path env: none — STATIC_DIR is a compile-time const (main.rs:58).

Replacement options grounded in these anchors: (a) keep a tiny static dir with a redirect index.html (target = `http://mgr.localhost/` or the sandbox's workspace URL); (b) replace the web-builder stage with a plain `COPY app/redirect/ /app/static` layer; (c) swap ServeDir for a redirect handler. Note the fallback + `/api` seam logic must stay (main.rs:176-185). Also: `docker-compose.yml` (repo stack) references and `Makefile` targets referencing web/ were not exhaustively audited for this note — check `make` targets + CI when writing design.md.

Also relevant: `web/` deletion impacts `app/Dockerfile` dep-cache? No — the npm project is not a cargo member (same as mgr-web per mgr/Dockerfile:35-36 comment). But `app/Dockerfile` build context is the repo root and COPYs `web/` — those lines must change.

## 6. PI_WEB_URL / MGR_URL env handling

### PI_WEB_URL (config.rs)

- `piweb_url_override()` (app/src/config.rs:251-253): `std::env::var("PI_WEB_URL").ok().filter(|v| !v.is_empty())` — empty string = unset.
- Consumed ONLY at startup in main.rs:84-97: for the `piWeb` service, a set value REPLACES `s.url` verbatim (no `{env:...}` expansion, no `{host}` left for the client — IframePane's replace is a no-op on a `{host}`-free string).
- Set by mgr's composegen: `PI_WEB_URL: http://sbx-{name}-piweb.mgr.localhost/` (mgr/src/composegen.rs:85) — the total-gateway subdomain URL (契约 3).
- Stock stacks: docker-compose publishes 30141 and the manifest keeps `http://{host}:{env:PI_WEB_HOST_PORT:30141}/`.
- `PI_WEB_ALLOWED_HOSTS`: consumed by pi-web's own request-security (baked profile + entrypoint default "app"; mgr compose sets `app,sbx-<name>-piweb.mgr.localhost` — composegen.rs:86, entrypoint.sh:23-28).

### MGR_URL (config.rs + main.rs + composegen.rs)

- `mgr_url()` (app/src/config.rs:264-266): same set/non-empty pattern; resolved once at startup.
- Carried on AppState (app/src/state.rs:37); consumed by:
  - `main.rs:111-113` — spawn mgr_sync when Some.
  - `routes/models::managed_guard` (mod.rs:74-79) — 403 `managed-by-mgr` on all 6 write endpoints when set (PUT config, import/pi, apply/:agent, provider PUT/DELETE, sync — 契约 7 matrix).
  - `GET /api/models/managed` (mod.rs:66-69) — `{"managed": bool}` for the frontend probe.
- Injected by mgr's composegen: `MGR_URL: http://mgr-api:8089` (mgr/src/composegen.rs:91); `mgr-api` is the aio-mgr-net alias of the mgr-api service (mgr/compose.yml:76-78).

## 7. Caveats / Not Found

- The repo-stack `docker-compose.yml` (app service env / published ports) was not re-read line-by-line for this note; PI_WEB_HOST_PORT passthrough documented in services.toml:91-100 and api-contracts.md (manifest placeholder section lines 162-199).
- `app/src/routes/manifest.rs` (thin handler) not quoted — it merges builtin+buttons, resolves PATH, calls build_manifest; the logic of interest is all in config.rs (quoted above).
- No existing "redirect page" artifact exists anywhere — this is net-new under D1.
- web/smoke-test.cjs + Makefile + CI references to web/ not audited (design.md should list them as removal surface).
