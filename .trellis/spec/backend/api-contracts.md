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
