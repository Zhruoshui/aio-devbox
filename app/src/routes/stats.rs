// GET /api/stats - container-self resource snapshot for the statusbar footer.
//
// Semantics are strictly the CONTAINER's view, not the host's (the host may be
// Windows/Linux, so cgroup v2 numbers keep the meaning portable):
//   - CPU%: `cpu.stat` `usage_usec` delta between samples, divided by wall-time
//     delta times the effective CPU budget (`cpu.max` quota/period when set,
//     otherwise available_parallelism). 0-100 scale. First sample has no delta
//     and reports 0.
//   - MEM: `memory.current` minus the `inactive_file` page cache - the same
//     accounting `docker stats` uses, so the footer matches what the operator
//     sees elsewhere. Without this subtraction evictable file cache (build
//     artifacts etc.) inflates "used" hugely. `memory.max` == "max" (no cgroup
//     limit; current compose sets none) => memTotalBytes = null and the UI
//     shows the absolute usage only.
//   - DISK: statvfs of the workspace volume mount point `/home/gem`.
//
// A background sampler (spawn_stats_sampler, 2s period) keeps
// AppState.stats fresh so the handler is a lock-free-ish clone - no cgroup
// reads on the request path. Any read/parse failure keeps the previous value
// and warns once-ish: the endpoint always returns 200 (the footer is
// decorative; a 5xx would be worse than a stale number).

use std::time::Instant;

use axum::{extract::State, Json};
use nix::sys::statvfs::statvfs;
use serde::Serialize;

use crate::state::AppState;

/// Serialized shape is the contract mirrored by `web/src/types.ts`.
// camelCase on purpose: the JSON field names ARE the API contract.
#[allow(non_snake_case)]
#[derive(Serialize, Clone, Debug, Default)]
pub struct StatsSnapshot {
    pub cpuPct: f64,
    pub memUsedBytes: u64,
    pub memTotalBytes: Option<u64>,
    pub diskUsedBytes: u64,
    pub diskTotalBytes: u64,
}

pub async fn stats(State(state): State<AppState>) -> Json<StatsSnapshot> {
    Json(state.stats.read().await.clone())
}

/// Spawn the 2s background sampler. Runs forever; errors never propagate.
pub fn spawn_stats_sampler(state: AppState) {
    tokio::spawn(async move {
        let mut prev: Option<(u64, Instant)> = None; // (usage_usec, when)
        loop {
            let mut snap = state.stats.read().await.clone();

            if let Some(usage) = read_cpu_usage_usec() {
                let now = Instant::now();
                if let Some((prev_usage, prev_at)) = prev {
                    let dt = now.duration_since(prev_at).as_secs_f64();
                    if dt > 0.0 {
                        let cpus = effective_cpus();
                        let du = (usage.saturating_sub(prev_usage)) as f64 / 1_000_000.0;
                        snap.cpuPct = ((du / dt) / cpus * 100.0).clamp(0.0, 100.0);
                    }
                }
                prev = Some((usage, now));
            } else {
                tracing::warn!("stats: failed to read cgroup cpu.stat, keeping last cpuPct");
            }

            match read_mem() {
                Some((used, total)) => {
                    snap.memUsedBytes = used;
                    snap.memTotalBytes = total;
                }
                None => tracing::warn!("stats: failed to read cgroup memory, keeping last values"),
            }

            match read_disk() {
                Some((used, total)) => {
                    snap.diskUsedBytes = used;
                    snap.diskTotalBytes = total;
                }
                None => tracing::warn!("stats: statvfs(/home/gem) failed, keeping last values"),
            }

            *state.stats.write().await = snap;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });
}

/// `cpu.stat`'s `usage_usec <n>` line (cumulative microseconds).
fn read_cpu_usage_usec() -> Option<u64> {
    let text = std::fs::read_to_string("/sys/fs/cgroup/cpu.stat").ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("usage_usec ") {
            return rest.trim().parse().ok();
        }
    }
    None
}

/// Effective CPU budget: `cpu.max` is `"<quota> <period>"` (or `"max ..."`).
fn effective_cpus() -> f64 {
    if let Ok(text) = std::fs::read_to_string("/sys/fs/cgroup/cpu.max") {
        let mut parts = text.split_whitespace();
        if let (Some(quota), Some(period)) = (parts.next(), parts.next()) {
            if let (Ok(q), Ok(p)) = (quota.parse::<f64>(), period.parse::<f64>()) {
                if p > 0.0 {
                    return q / p;
                }
            }
        }
    }
    std::thread::available_parallelism().map(|n| n.get() as f64).unwrap_or(1.0)
}

/// (used, total) from cgroup v2 memory files. total is None when unlimited.
fn read_mem() -> Option<(u64, Option<u64>)> {
    let current: u64 = std::fs::read_to_string("/sys/fs/cgroup/memory.current")
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let inactive = std::fs::read_to_string("/sys/fs/cgroup/memory.stat")
        .ok()
        .and_then(|text| {
            text.lines().find_map(|l| {
                l.strip_prefix("inactive_file ").and_then(|v| v.trim().parse::<u64>().ok())
            })
        })
        .unwrap_or(0);
    let total = match std::fs::read_to_string("/sys/fs/cgroup/memory.max") {
        Ok(text) if text.trim() != "max" => text.trim().parse::<u64>().ok(),
        _ => None,
    };
    Some((current.saturating_sub(inactive), total))
}

/// (used, total) bytes for the workspace volume via statvfs.
fn read_disk() -> Option<(u64, u64)> {
    let vfs = statvfs(std::path::Path::new("/home/gem")).ok()?;
    let frag = vfs.fragment_size() as u64;
    let total = vfs.blocks() as u64 * frag;
    let used = (vfs.blocks() as u64 - vfs.blocks_free() as u64) * frag;
    Some((used, total))
}
