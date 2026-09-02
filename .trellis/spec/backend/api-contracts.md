# API Contracts

Executable contracts for the axum HTTP surface. Code owners: the
serialization struct in `app/src/routes/*.rs` is the single owner of each
payload; the frontend mirror lives in `web/src/types.ts` (see
cross-layer-thinking-guide: decode once at the boundary).

## GET /api/stats — container-view resource metrics

### 1. Scope / Trigger

Added by the footer-usability task (`.trellis/tasks/08-26-footer-usability/`).
Drives the statusbar resource readout and doubles as the backend heartbeat for
the connection dot. Any field change here is a cross-layer contract change:
update both code owners and the frontend renderer together.

### 2. Signatures

- Route: `GET /api/stats`, registered in `main.rs` **before** the
  `/api/*rest` seam catch-all (static segments win, same as `/api/manifest`).
- Backend owner: `app/src/routes/stats.rs::StatsSnapshot`
  (`#[allow(non_snake_case)]` — the camelCase field names ARE the wire
  contract; do not rename casually).
- Frontend mirror: `web/src/types.ts::StatsSnapshot` (`memTotalBytes?`).
- Data source: `spawn_stats_sampler` (tokio task, 2s period) keeps the
  snapshot in `AppState.stats`; the handler only clones-and-returns — no
  cgroup reads on the request path.

### 3. Contracts

Response JSON (always 200):

```json
{
  "cpuPct": 12.4,
  "memUsedBytes": 1300000000,
  "memTotalBytes": null,
  "diskUsedBytes": 8500000000,
  "diskTotalBytes": 62000000000
}
```

- `cpuPct`: f64 0–100. Container CPU vs effective quota: cgroup v2
  `cpu.stat` `usage_usec` delta / (Δt × cpus_eff), where cpus_eff =
  `cpu.max` quota/period when limited, else `available_parallelism()`.
  First sample after boot is 0 (usage_usec is cumulative — a delta needs two
  reads).
- `memUsedBytes`: `memory.current − memory.stat inactive_file` — the same
  accounting `docker stats` uses. Raw `memory.current` includes page cache
  and reads far too high.
- `memTotalBytes`: the cgroup `memory.max` limit; `null` when `max` (no
  limit — the current compose default). The frontend renders absolute-only
  then (`MEM 336.5M`, no denominator).
- `diskUsedBytes` / `diskTotalBytes`: `statvfs("/home/gem")` — the workspace
  volume, not the container overlay.

All sources are **container-view** by design (user decision: the host may be
Windows/macOS/Linux; container semantics are uniform). Verified to match
`docker stats --no-stream` (mem, within sampling drift) and `df -B1 /home/gem`
(disk, exact).

### 4. Validation & Error Matrix

- cgroup/statvfs read or parse fails → `tracing::warn!`, keep the previous
  field value (first failure: 0 / `None`); endpoint still returns 200.
  Never 5xx, never panic — the footer is advisory.
- Backend unreachable from the frontend → `useStats` sets `online: false`:
  stats seg hidden, statusbar dot red, `statusOffline` text. Recovery is
  automatic on the next 3s poll.

### 5. Good/Base/Bad Cases

- Good: `{"cpuPct": 3.1, ..., "memTotalBytes": 4294967296}` — compose sets a
  memory limit; frontend shows `MEM 1.2G / 4G`.
- Base: `memTotalBytes: null` — unlimited; frontend shows `MEM 336.5M`.
- Bad: reading `memory.current` alone as "used" (page cache inflates it);
  returning 500 when the sampler hiccups; computing cpuPct from a single
  `cpu.stat` read.

### 6. Tests Required

- `web/smoke-test.cjs`: `.statusbar .seg-stats` renders and its text contains
  CPU / MEM / DISK (assertion `statsOk`).
- Manual cross-check on change: endpoint vs `docker stats --no-stream` and
  `df -B1 /home/gem` inside the app container.

### 7. Wrong vs Correct

Wrong: frontend derives "offline" from the manifest fetch only (refresh
failures are deliberately swallowed there).

Correct: the 3s `/api/stats` poll is the heartbeat — any failure flips
`online` false within one period; the manifest channel stays responsible for
button visibility only. Poll (3s) and sample (2s) periods are coprime so the
readout does not beat against the sampler.

## POST/DELETE /api/buttons + GET /preview/:port — user web-type buttons & dev-server preview

### 1. Scope / Trigger

Added by the web-button-preview task (`.trellis/tasks/09-01-web-button-preview/`,
issue #1). `POST /api/buttons` previously created agent buttons only; it now
accepts `type: "web"` + `port`, and a new dynamic reverse proxy serves
`/preview/<port>/*` from the app (axum) — NOT the gateway (a Caddy route cannot
reach a loopback-bound dev server; the gateway's catch-all already hands
`/preview/*` to axum, so Caddyfile/compose stay untouched).

### 2. Signatures

- `POST /api/buttons` — owner: `app/src/routes/buttons.rs::ButtonInput`
  (`{label, cmd?, type?, port?}`; `type` defaults to `"agent"`). Web buttons
  persist `cmd: ""` + `port: u16` in `buttons.toml`
  (`ButtonDef.port: Option<u16>`, `skip_serializing_if` — old files deserialize
  unchanged; agent rows omit the key).
- `GET /api/manifest` — unchanged shape; web user buttons emit
  `url: "/preview/<port>/"`, `target: "127.0.0.1:<port>"` (TCP probe, same
  semantics as built-in web buttons). Owner: `config.rs::load_buttons`
  (web-without-port rows are dropped with a warn, never a dead pane).
- `GET /preview/:port(/:path)` — owner: `app/src/routes/preview.rs`
  (`preview_proxy`). Routes: `/preview/:port`, `/preview/:port/`,
  `/preview/:port/*path` (matchit 0.7.3 catch-all needs a non-empty tail).
- `GET /api/buttons/probe?port=<N>` — added by 09-02-web-button-ux-fix.
  Owner: `app/src/routes/buttons.rs::probe_port`. TCP-dials `127.0.0.1:<port>`
  (same 400ms timeout as the manifest liveness probe in `config.rs`), returns
  `200 {"listening": bool}`. Port rules mirror POST validation (integer
  1-65535, 0/8088/non-numeric → 400). Non-blocking UX hint for the register
  dialog: `listening:false` is a warning, never an error — registration of a
  dead port stays allowed.
- Frontend mirror: `web/src/types.ts::RegisterButtonInput`.

### 3. Contracts

Validation matrix (POST, 400 on violation): web without port; port 0;
port 8088 (axum itself — proxying would recurse); unknown `type`; agent with
empty/oversized cmd. Web rows normalize `cmd` to `""`.

Proxy behavior: HTTP all-methods forwarded (hop-by-hop headers stripped, Host
rewritten to `127.0.0.1:<port>`), response bodies streamed unbuffered
(reqwest `bytes_stream` → `Body::from_stream`; SSE survives). WS upgrades are
detected via `Option<WebSocketUpgrade>` + the Upgrade header, connected with
tokio-tungstenite (plaintext, `WS_CONNECT_TIMEOUT` 2s), pumped message-level
both ways until either side closes; the upstream-negotiated
`Sec-WebSocket-Protocol` is echoed back to the browser (vite HMR negotiates
`vite-hmr` and aborts without it). Errors: upstream unreachable → 502;
non-numeric port / port 0 / 8088 → 404 (fast-fail, no proxy attempt).

### 4. Tests Required

- Unit: `config.rs` (web parsing, legacy no-port file, web-without-port drop),
  `buttons.rs::validate_shape` matrix, `preview.rs` pure fns
  (`port_allowed`, `upstream_path`, hop-by-hop filter).
- Focused integration (issue #1): register → manifest → proxy HTTP/SSE/WS →
  delete, 21 assertions (`/tmp/preview-itest/setup.sh` harness).

### 5. Wrong vs Correct

Wrong: proxying 8088 (self-recursion `/preview/8088/preview/...`), rewriting
HTML to fix root-absolute asset URLs (vite/Next need upstream `base` config —
documented in README), stripping the WS subprotocol on the way back.
