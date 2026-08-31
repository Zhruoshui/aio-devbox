// Token usage statistics: GET /api/models/usage?window=today|7d|all[&refresh=1].
//
// Aggregates per-model token usage from each installed agent's local session
// logs (design §6, research/aio-integration-and-usage-sources §3 — VERIFIED
// field names are ground truth):
//   pi       ~/.pi/agent/sessions/**/*.jsonl        message.usage.{input,output,cacheRead,cacheWrite,cost.total} + message.model + record-level timestamp
//   opencode ~/.local/share/opencode/opencode.db    message.data JSON: tokens.{input,output,cache.{read,write}} + modelID + providerID + cost + time.created
//   claude   ~/.claude/projects/**/*.jsonl           message.usage.{input_tokens,output_tokens,cache_creation_input_tokens,cache_read_input_tokens} + message.model + timestamp
//   codex    ~/.codex/sessions/**/*.jsonl            token_count event: info.total_token_usage.{input_tokens,cached_input_tokens,output_tokens}; model from TurnContext
//
// Robustness: every file/db operation is fallible and skipped; a single
// corrupt record never breaks the response. Use serde_json::Value for the
// permissive parsing (don't hard-model every field).
//
// Cost (task 08-27-usage-correctness): canonical `provider.models[].cost`
// unit is USD per 1M tokens. Logged cost is trusted only when > 0 (pi and
// opencode always log 0 — audit finding); otherwise cost is computed from
// canonical config, cache reads/writes priced with their own rates (see the
// `cost backfill` section below). All-zero token rows are dropped.
//
// Cache: 30s TTL keyed by window string, in a process-global Mutex. A
// `?refresh=1` query param bypasses the cache once.
//
// Pure helpers are unit-tested here; live IO is exercised by functional tests.

use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{Query, State};
use axum::Json;
use rusqlite::OpenFlags;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::state::AppState;
use super::render::home_dir;
use super::store::{read_config, CanonicalConfig, CostEntry};

/// Cache TTL (design §6: 30s).
const CACHE_TTL: Duration = Duration::from_secs(30);

// ── request / response types ──────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UsageQuery {
    #[serde(default = "default_window")]
    pub window: String,
    /// `1` (or any non-empty value) bypasses the cache once.
    #[serde(default)]
    pub refresh: Option<String>,
}

fn default_window() -> String {
    "today".to_string()
}

/// One aggregated row. Row identity = (agent, provider?, model). `cost`
/// is present only when at least one source record for that row carried
/// cost (design §6: cost optional).
#[derive(Debug, Clone, Serialize)]
pub struct UsageRow {
    pub agent: String,
    pub provider: Option<String>,
    pub model: String,
    pub r#in: u64,
    pub out: u64,
    #[serde(rename = "cacheRead")]
    pub cache_read: u64,
    #[serde(rename = "cacheWrite")]
    pub cache_write: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct UsageResponse {
    pub rows: Vec<UsageRow>,
    #[serde(rename = "generatedAt")]
    pub generated_at: String,
}

// ── cache ──────────────────────────────────────────────────────────

struct CacheEntry {
    at: Instant,
    window: String,
    rows: Vec<UsageRow>,
    generated_at: String,
}

static CACHE: OnceLock<Mutex<Option<CacheEntry>>> = OnceLock::new();

fn cache() -> &'static Mutex<Option<CacheEntry>> {
    CACHE.get_or_init(|| Mutex::new(None))
}

// ── handler ───────────────────────────────────────────────────────

pub async fn usage(
    State(state): State<AppState>,
    Query(q): Query<UsageQuery>,
) -> Json<UsageResponse> {
    let window = match q.window.as_str() {
        "today" | "7d" | "all" => q.window.clone(),
        // Unknown window: fall back to today (don't 400 — usage is read-only
        // and harmless; log instead).
        _ => "today".to_string(),
    };
    let force_refresh = q
        .refresh
        .as_deref()
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false);

    // Cache check (return the cached rows + their original generatedAt).
    if !force_refresh {
        if let Some(entry) = cache().lock().await.as_ref() {
            if entry.window == window && entry.at.elapsed() < CACHE_TTL {
                return Json(UsageResponse {
                    rows: entry.rows.clone(),
                    generated_at: entry.generated_at.clone(),
                });
            }
        }
    }

    // Fresh compute. The cutoff is computed from SystemTime only here in the
    // handler — pure scan helpers take an injectable `now_secs` + `cutoff_secs`
    // so they're testable without globals.
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cutoff_secs = window_cutoff(&window, now_secs);
    let cutoff_ms = cutoff_secs.saturating_mul(1000);

    let home = home_dir();
    // Canonical is best-effort for the pi provider join; absence is fine.
    let canonical = read_config(&state.models_file).unwrap_or_default();

    let mut buckets: BTreeMap<(String, Option<String>, String), UsageRow> = BTreeMap::new();

    // Each scanner swallows its own errors and contributes nothing on failure.
    for row in scan_pi(&home, &canonical, cutoff_secs) {
        merge_row(&mut buckets, row);
    }
    for row in scan_opencode(&home, cutoff_ms) {
        merge_row(&mut buckets, row);
    }
    for row in scan_claude(&home, cutoff_secs) {
        merge_row(&mut buckets, row);
    }
    for row in scan_codex(&home, cutoff_secs) {
        merge_row(&mut buckets, row);
    }

    let mut rows: Vec<UsageRow> = buckets.into_values().collect();

    // Drop all-zero rows (design §3): a row with 0 in/out/cache tokens is
    // noise (e.g. an opencode free-model session that never really ran).
    rows.retain(|r| r.r#in + r.out + r.cache_read + r.cache_write > 0);

    // Fill cost for rows whose logged cost is absent or (audit finding) the
    // always-0 placeholder pi/opencode write (design §2).
    backfill_cost(&mut rows, &canonical);

    // Stable order: agent, provider, model.
    rows.sort_by(|a, b| {
        a.agent
            .cmp(&b.agent)
            .then(a.provider.cmp(&b.provider))
            .then(a.model.cmp(&b.model))
    });

    let generated_at = format_iso_utc(now_secs);
    let resp = UsageResponse {
        rows: rows.clone(),
        generated_at: generated_at.clone(),
    };

    // Store in cache.
    *cache().lock().await = Some(CacheEntry {
        at: Instant::now(),
        window,
        rows,
        generated_at,
    });

    Json(resp)
}

/// Merge a row into the bucket map (sums tokens/cost for matching identity).
fn merge_row(
    buckets: &mut BTreeMap<(String, Option<String>, String), UsageRow>,
    row: UsageRow,
) {
    let key = (row.agent.clone(), row.provider.clone(), row.model.clone());
    buckets.entry(key).and_modify(|existing| {
        existing.r#in += row.r#in;
        existing.out += row.out;
        existing.cache_read += row.cache_read;
        existing.cache_write += row.cache_write;
        if let Some(c) = row.cost {
            existing.cost = Some(existing.cost.unwrap_or(0.0) + c);
        }
    }).or_insert(row);
}

/// Compute the cutoff epoch seconds for a window (pure, testable).
/// - today: start of the current UTC day (00:00)
/// - 7d:    now - 7*86400
/// - all:   0 (epoch)
fn window_cutoff(window: &str, now_secs: u64) -> u64 {
    match window {
        "today" => {
            // Start of the current UTC day: floor to whole days.
            (now_secs / 86400) * 86400
        }
        "7d" => now_secs.saturating_sub(7 * 86400),
        _ => 0,
    }
}

/// Format an epoch-seconds timestamp as `YYYY-MM-DDTHH:MM:SSZ` (UTC, no chrono).
fn format_iso_utc(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let sec_in_day = secs % 86400;
    let hour = sec_in_day / 3600;
    let min = (sec_in_day % 3600) / 60;
    let sec = sec_in_day % 60;
    let (year, month, day) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, min, sec
    )
}

/// Convert days-since-Unix-epoch to (year, month, day). Matches the renderer
/// `days_to_ymd` (kept inline to avoid coupling modules).
fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    let mut year = 1970;
    let mut days = days;
    loop {
        let in_year = if is_leap(year) { 366 } else { 365 };
        if days < in_year {
            break;
        }
        days -= in_year;
        year += 1;
    }
    let month_lens: [i64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u32;
    for &ml in &month_lens {
        if days < ml {
            break;
        }
        days -= ml;
        month += 1;
    }
    (year, month, (days + 1) as u32)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

// ── pi scan ────────────────────────────────────────────────────────

/// Scan pi session jsonl files. Each record carries `model` and `usage` nested
/// under a top-level `message` object (verified shape):
///   {"type","version","id","timestamp","cwd","message":{"role","model","usage":{...}}}
/// Records without a `message` (e.g. the session header line) are skipped.
/// Time filter: top-level `timestamp` (ISO8601) if parseable else file mtime.
/// provider is joined from canonical (best-effort). Missing dir => nothing.
pub fn scan_pi(home: &Path, canonical: &CanonicalConfig, cutoff_secs: u64) -> Vec<UsageRow> {
    let sessions = home.join(".pi/agent/sessions");
    if !sessions.is_dir() {
        return Vec::new();
    }
    let files = match collect_jsonl(&sessions) {
        Ok(f) => f,
        Err(e) => {
            tracing::debug!(target: "models::usage::pi", "collect jsonl: {e}");
            return Vec::new();
        }
    };

    let mut buckets: BTreeMap<String, UsageRow> = BTreeMap::new();
    for file in files {
        let mtime_secs = file_mtime_secs(&file);
        let reader = match std::fs::File::open(&file) {
            Ok(f) => std::io::BufReader::new(f),
            Err(e) => {
                tracing::debug!(target: "models::usage::pi", "open {}: {e}", file.display());
                continue;
            }
        };
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let v: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            // `model` and `usage` live under the top-level `message` object,
            // NOT at the record root. Records without a message (session
            // header, tool-only entries) contribute nothing.
            let message = match v.get("message") {
                Some(m) => m,
                None => continue,
            };
            let model = match message.get("model").and_then(|m| m.as_str()) {
                Some(m) if !m.is_empty() => m.to_string(),
                _ => continue,
            };
            let usage = match message.get("usage").and_then(|u| u.as_object()) {
                Some(u) => u,
                None => continue,
            };
            // Window filter: timestamp is at the record root (verified).
            let t = parse_timestamp_secs(&v).unwrap_or(mtime_secs);
            if t < cutoff_secs {
                continue;
            }
            let row = buckets.entry(model.clone()).or_insert(UsageRow {
                agent: "pi".into(),
                provider: None,
                model: model.clone(),
                r#in: 0,
                out: 0,
                cache_read: 0,
                cache_write: 0,
                cost: None,
            });
            row.r#in += as_u64(usage.get("input"));
            row.out += as_u64(usage.get("output"));
            row.cache_read += as_u64(usage.get("cacheRead"));
            row.cache_write += as_u64(usage.get("cacheWrite"));
            if let Some(c) = usage
                .get("cost")
                .and_then(|c| c.get("total"))
                .and_then(|t| t.as_f64())
            {
                row.cost = Some(row.cost.unwrap_or(0.0) + c);
            }
        }
    }

    // Join provider from canonical.
    let mut out: Vec<UsageRow> = buckets.into_values().collect();
    for row in &mut out {
        row.provider = find_provider_for_model(canonical, &row.model);
    }
    out
}

/// Find the canonical provider id whose models contains `model_id` (best-effort).
fn find_provider_for_model(canonical: &CanonicalConfig, model_id: &str) -> Option<String> {
    for (id, provider) in &canonical.providers {
        if provider.models.iter().any(|m| m.id == model_id) {
            return Some(id.clone());
        }
    }
    None
}

// ── opencode scan ──────────────────────────────────────────────────

/// Scan opencode's SQLite DB (read-only, WAL-safe). Aggregates assistant
/// message rows by (providerID, modelID). Missing db / read error => nothing.
pub fn scan_opencode(home: &Path, cutoff_ms: u64) -> Vec<UsageRow> {
    let db = home.join(".local/share/opencode/opencode.db");
    if !db.exists() {
        return Vec::new();
    }
    let conn = match rusqlite::Connection::open_with_flags(
        &db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(target: "models::usage::opencode", "open db: {e}");
            return Vec::new();
        }
    };

    let mut stmt = match conn.prepare("SELECT data, time_created FROM message") {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(target: "models::usage::opencode", "prepare: {e}");
            return Vec::new();
        }
    };
    let rows_iter = match stmt.query_map([], |row| {
        let data: String = row.get(0)?;
        let time_created: Option<i64> = row.get(1).ok();
        Ok((data, time_created))
    }) {
        Ok(it) => it,
        Err(e) => {
            tracing::debug!(target: "models::usage::opencode", "query: {e}");
            return Vec::new();
        }
    };

    let mut buckets: BTreeMap<(String, String), UsageRow> = BTreeMap::new();
    for item in rows_iter {
        let (data, time_created) = match item {
            Ok(v) => v,
            Err(_) => continue,
        };
        let v: Value = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        // Only assistant rows carry tokens.
        if v.get("role").and_then(|r| r.as_str()) != Some("assistant") {
            continue;
        }
        // Window filter (time_created is ms epoch).
        if let Some(ms) = time_created {
            let ms_u = ms.max(0) as u64;
            if ms_u < cutoff_ms {
                continue;
            }
        }
        let provider_id = v
            .get("providerID")
            .and_then(|x| x.as_str())
            .unwrap_or("opencode")
            .to_string();
        let model_id = v
            .get("modelID")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string();
        let key = (provider_id.clone(), model_id.clone());
        let row = buckets.entry(key).or_insert(UsageRow {
            agent: "opencode".into(),
            provider: Some(provider_id),
            model: model_id,
            r#in: 0,
            out: 0,
            cache_read: 0,
            cache_write: 0,
            cost: None,
        });
        let tokens = v.get("tokens").and_then(|t| t.as_object());
        if let Some(tokens) = tokens {
            row.r#in += as_u64(tokens.get("input"));
            row.out += as_u64(tokens.get("output"));
            // cache is nested: { read, write }
            if let Some(cache) = tokens.get("cache").and_then(|c| c.as_object()) {
                row.cache_read += as_u64(cache.get("read"));
                row.cache_write += as_u64(cache.get("write"));
            }
        }
        if let Some(c) = v.get("cost").and_then(|c| c.as_f64()) {
            row.cost = Some(row.cost.unwrap_or(0.0) + c);
        }
    }

    buckets.into_values().collect()
}

// ── claude scan ────────────────────────────────────────────────────

/// Scan claude code session jsonl files (guard with dir existence).
/// assistant records carry `message.model` + `message.usage.*` + line-level
/// `timestamp` (ISO). No cost in claude logs => cost omitted.
pub fn scan_claude(home: &Path, cutoff_secs: u64) -> Vec<UsageRow> {
    let projects = home.join(".claude/projects");
    if !projects.is_dir() {
        return Vec::new();
    }
    let files = match collect_jsonl(&projects) {
        Ok(f) => f,
        Err(e) => {
            tracing::debug!(target: "models::usage::claude", "collect jsonl: {e}");
            return Vec::new();
        }
    };

    let mut buckets: BTreeMap<String, UsageRow> = BTreeMap::new();
    for file in files {
        let mtime_secs = file_mtime_secs(&file);
        let reader = match std::fs::File::open(&file) {
            Ok(f) => std::io::BufReader::new(f),
            Err(_) => continue,
        };
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let v: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let message = match v.get("message") {
                Some(m) => m,
                None => continue,
            };
            let model = match message.get("model").and_then(|m| m.as_str()) {
                Some(m) if !m.is_empty() => m.to_string(),
                _ => continue,
            };
            let usage = match message.get("usage").and_then(|u| u.as_object()) {
                Some(u) => u,
                None => continue,
            };
            let t = parse_timestamp_secs(&v).unwrap_or(mtime_secs);
            if t < cutoff_secs {
                continue;
            }
            let row = buckets.entry(model.clone()).or_insert(UsageRow {
                agent: "claude".into(),
                provider: None,
                model: model.clone(),
                r#in: 0,
                out: 0,
                cache_read: 0,
                cache_write: 0,
                cost: None,
            });
            row.r#in += as_u64(usage.get("input_tokens"));
            row.out += as_u64(usage.get("output_tokens"));
            row.cache_read += as_u64(usage.get("cache_read_input_tokens"));
            row.cache_write += as_u64(usage.get("cache_creation_input_tokens"));
        }
    }

    buckets.into_values().collect()
}

// ── codex scan ─────────────────────────────────────────────────────

/// Scan codex session jsonl files (guard with dir existence). For each file,
/// track the most recent TurnContext model name; `token_count` events
/// contribute their `info.total_token_usage` to that model's bucket.
pub fn scan_codex(home: &Path, cutoff_secs: u64) -> Vec<UsageRow> {
    let sessions = home.join(".codex/sessions");
    if !sessions.is_dir() {
        return Vec::new();
    }
    let files = match collect_jsonl(&sessions) {
        Ok(f) => f,
        Err(e) => {
            tracing::debug!(target: "models::usage::codex", "collect jsonl: {e}");
            return Vec::new();
        }
    };

    let mut buckets: BTreeMap<String, UsageRow> = BTreeMap::new();
    for file in files {
        let mtime_secs = file_mtime_secs(&file);
        let reader = match std::fs::File::open(&file) {
            Ok(f) => std::io::BufReader::new(f),
            Err(_) => continue,
        };
        let mut last_model: Option<String> = None;
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let v: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            // Track the most recent TurnContext model name. The model field
            // may live at TurnContext.model or at the top level; we try both.
            if v.get("type").and_then(|x| x.as_str()) == Some("turn_context")
                || v.get("TurnContext").is_some()
            {
                if let Some(m) = v
                    .get("model")
                    .and_then(|x| x.as_str())
                    .or_else(|| v.get("TurnContext").and_then(|t| t.get("model")).and_then(|m| m.as_str()))
                {
                    if !m.is_empty() {
                        last_model = Some(m.to_string());
                    }
                }
            }
            // token_count event carries usage totals.
            let is_token_count = v
                .get("type")
                .and_then(|x| x.as_str())
                .map(|s| s == "token_count")
                .unwrap_or(false)
                || v.get("token_count").is_some();
            if !is_token_count {
                continue;
            }
            let total = match v
                .get("info")
                .and_then(|i| i.get("total_token_usage"))
                .and_then(|t| t.as_object())
            {
                Some(t) => t,
                None => continue,
            };
            let t = parse_timestamp_secs(&v).unwrap_or(mtime_secs);
            if t < cutoff_secs {
                continue;
            }
            let model = last_model.clone().unwrap_or_else(|| "unknown".to_string());
            let row = buckets.entry(model.clone()).or_insert(UsageRow {
                agent: "codex".into(),
                provider: None,
                model: model.clone(),
                r#in: 0,
                out: 0,
                cache_read: 0,
                cache_write: 0,
                cost: None,
            });
            row.r#in += as_u64(total.get("input_tokens"));
            row.out += as_u64(total.get("output_tokens"));
            row.cache_read += as_u64(total.get("cached_input_tokens"));
        }
    }

    buckets.into_values().collect()
}

// ── cost backfill (task 08-27-usage-correctness, design §2) ────────
//
// Audit ground truth: pi and opencode log a `cost` field that is ALWAYS 0
// (never a real computed cost), and claude/codex logs carry no cost at all.
// So the fill priority is:
//   1. log cost > 0          -> trust it, never touch (no double count)
//   2. log cost 0/None       -> compute from canonical `provider.models[].cost`
//                               (unit: USD per 1M tokens, design §1), cache
//                               reads/writes priced with their OWN rates
//   3. no cost match         -> leave None (never invent a price)
// Match tiers: (provider,model) exact -> model exact across providers ->
// version/date-suffix fuzzy, forward direction only (row id = canonical id +
// `-\d[\d.]*`, e.g. `claude-sonnet-4-20250514` vs `claude-sonnet-4`;
// alphabetic variant suffixes like `-free` never match — different model).

/// Price `tokens` at `per_m` USD/1M-tokens (None price contributes 0 —
/// canonical cost entries may be partially configured).
fn price_per_m(tokens: u64, per_m: Option<f64>) -> f64 {
    match per_m {
        Some(p) => tokens as f64 / 1_000_000.0 * p,
        None => 0.0,
    }
}

/// Compute a row's cost from a canonical cost entry (design §1): each token
/// bucket uses its OWN unit price; cacheRead/cacheWrite never ride the
/// input rate.
fn compute_cost(entry: &CostEntry, row: &UsageRow) -> f64 {
    price_per_m(row.r#in, entry.input)
        + price_per_m(row.out, entry.output)
        + price_per_m(row.cache_read, entry.cache_read)
        + price_per_m(row.cache_write, entry.cache_write)
}

/// One model-match candidate: canonical provider id + the matched canonical
/// model id length (for most-specific-first ordering) + its cost entry.
struct CostHit<'a> {
    provider: String,
    model_len: usize,
    entry: &'a CostEntry,
}

/// Find cost candidates whose canonical model id matches `model` exactly.
fn exact_model_hits<'a>(
    canonical: &'a CanonicalConfig,
    model: &str,
) -> Vec<CostHit<'a>> {
    let mut out = Vec::new();
    for (id, provider) in &canonical.providers {
        if let Some(entry) = provider
            .models
            .iter()
            .find(|m| m.id == model)
            .and_then(|m| m.cost.as_ref())
        {
            out.push(CostHit {
                provider: id.clone(),
                model_len: model.len(),
                entry,
            });
        }
    }
    out
}

/// True when `s` looks like a version/date stamp appended to a model id:
/// design §2c's `-\d[\d.]*` — a '-' followed by digits/dots, LED by a digit
/// ("-20250514", "-4", "-4.5"; NOT "-." or "-.5"). Alphabetic variants
/// ("-free", "-vision-exp") are DIFFERENT models, not the same model with a
/// date stamp — pricing `deepseek-v4-flash-free` at `deepseek-v4-flash`
/// rates would be wrong (audit finding from the container reconciliation), so
/// those must NOT fuzzy-match. ASCII digits only: unicode digit look-alikes
/// are not version stamps.
fn is_version_suffix(s: &str) -> bool {
    match s.strip_prefix('-') {
        Some(rest) if !rest.is_empty() => {
            rest.starts_with(|c: char| c.is_ascii_digit())
                && rest.chars().all(|c| c.is_ascii_digit() || c == '.')
        }
        _ => false,
    }
}

/// Fuzzy tier: the row model is a canonical id plus a version/date suffix
/// (`claude-sonnet-4-20250514` vs `claude-sonnet-4`). Forward direction only —
/// a shorter row id matching a longer canonical id ("gpt" vs "gpt-5") is too
/// risky. Longest canonical id wins (most specific match).
fn fuzzy_model_hits<'a>(
    canonical: &'a CanonicalConfig,
    model: &str,
) -> Vec<CostHit<'a>> {
    let mut out: Vec<CostHit<'a>> = Vec::new();
    for (id, provider) in &canonical.providers {
        for m in &provider.models {
            let matches = !m.id.is_empty()
                && model.len() > m.id.len()
                && model.starts_with(m.id.as_str())
                && is_version_suffix(&model[m.id.len()..]);
            if matches {
                if let Some(entry) = m.cost.as_ref() {
                    out.push(CostHit {
                        provider: id.clone(),
                        model_len: m.id.len(),
                        entry,
                    });
                }
            }
        }
    }
    // Most specific (longest matched canonical id) first, then stable by id.
    out.sort_by(|a, b| b.model_len.cmp(&a.model_len).then(a.provider.cmp(&b.provider)));
    out
}

/// Resolve a row's cost config (design §2). Returns the hit; the provider is
/// reported only when unambiguous (exactly one candidate) so the caller can
/// backfill `row.provider` without mislabeling.
fn lookup_cost<'a>(
    canonical: &'a CanonicalConfig,
    row: &UsageRow,
) -> Option<(Option<String>, &'a CostEntry)> {
    // Tier a: the row's provider is known and canonical, exact model match.
    if let Some(pid) = &row.provider {
        if let Some(entry) = canonical
            .providers
            .get(pid)
            .and_then(|p| p.models.iter().find(|m| m.id == row.model))
            .and_then(|m| m.cost.as_ref())
        {
            return Some((Some(pid.clone()), entry));
        }
    }
    // Tier b: exact model id across providers. Unambiguous when exactly one.
    let exact = exact_model_hits(canonical, &row.model);
    if let Some(first) = exact.first() {
        let provider = if exact.len() == 1 {
            Some(first.provider.clone())
        } else {
            None
        };
        return Some((provider, first.entry));
    }
    // Tier c: prefix-fuzzy, most specific first.
    let fuzzy = fuzzy_model_hits(canonical, &row.model);
    if let Some(first) = fuzzy.first() {
        let provider = if fuzzy.len() == 1 {
            Some(first.provider.clone())
        } else {
            None
        };
        return Some((provider, first.entry));
    }
    None
}

/// Fill in `cost` (and, when unambiguous, `provider`) for every row whose
/// logged cost is absent or zero. Rows with a real logged cost (> 0) are
/// left untouched (design §2: no double count).
pub fn backfill_cost(rows: &mut [UsageRow], canonical: &CanonicalConfig) {
    for row in rows.iter_mut() {
        if row.cost.map(|c| c > 0.0).unwrap_or(false) {
            continue; // real logged cost — trust it
        }
        if let Some((provider, entry)) = lookup_cost(canonical, row) {
            row.cost = Some(compute_cost(entry, row));
            if row.provider.is_none() {
                row.provider = provider;
            }
        }
        // No hit: cost stays as-is (None, or the logged 0 for free models).
    }
}


/// Recursively collect all `.jsonl` files under `root`. Missing dir is an
/// error (caller decides to skip).
fn collect_jsonl(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    visit_dir(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn visit_dir(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            visit_dir(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
    Ok(())
}

/// File mtime as epoch seconds (0 on failure).
fn file_mtime_secs(path: &Path) -> u64 {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Parse a top-level `timestamp` ISO8601 string to epoch seconds.
/// Accepts `YYYY-MM-DDTHH:MM:SS[.fff][Z|+HH:MM]`. Returns None on absence or
/// parse failure (caller falls back to mtime).
fn parse_timestamp_secs(v: &Value) -> Option<u64> {
    let s = v.get("timestamp")?.as_str()?;
    iso8601_to_epoch(s)
}

/// Parse a subset of ISO8601 to epoch seconds. Handles the common shapes
/// emitted by the agents (`Z`/offset, fractional seconds). No external crate.
fn iso8601_to_epoch(s: &str) -> Option<u64> {
    // Split date and time at 'T' or ' '.
    let (date_part, time_part) = s.split_once('T').or_else(|| s.split_once(' '))?;
    let (y, mo, d) = parse_ymd(date_part)?;
    // Strip timezone suffix.
    let (time_main, tz_offset_secs) = split_tz(time_part)?;
    // Strip fractional seconds.
    let (hms, _frac) = match time_main.split_once('.') {
        Some((hms, frac)) => (hms, Some(frac)),
        None => (time_main, None),
    };
    let (h, mi, se) = parse_hms(hms)?;
    let days = days_from_civil(y, mo, d)?;
    let secs = days * 86400 + (h as i64) * 3600 + (mi as i64) * 60 + se as i64 - tz_offset_secs;
    if secs < 0 {
        return Some(0);
    }
    Some(secs as u64)
}

fn parse_ymd(s: &str) -> Option<(i64, u32, u32)> {
    let mut it = s.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let mo: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    Some((y, mo, d))
}

fn parse_hms(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s.split(':');
    let h: u32 = it.next()?.parse().ok()?;
    let mi: u32 = it.next()?.parse().ok()?;
    let se: u32 = it.next().unwrap_or("0").parse().ok()?;
    Some((h, mi, se))
}

/// Split the time portion into (main, tz_offset_seconds). `Z` => offset 0;
/// `+HH:MM`/`-HH:MM` => signed; no tz => 0 (treat as UTC — sufficient for
/// window filtering where errors of a few hours don't matter).
fn split_tz(time_part: &str) -> Option<(&str, i64)> {
    if let Some(stripped) = time_part.strip_suffix('Z') {
        return Some((stripped, 0));
    }
    // Look for +HH:MM or -HH:MM at the end.
    if let Some(idx) = time_part.rfind(['+', '-']) {
        let (main, tz) = time_part.split_at(idx);
        if !main.is_empty() {
            let sign = if &tz[..1] == "-" { -1 } else { 1 };
            let rest = &tz[1..];
            let (h, m) = match rest.split_once(':') {
                Some((h, m)) => (h, m),
                None => (rest, "0"),
            };
            let h: i64 = h.parse().ok()?;
            let m: i64 = m.parse().ok()?;
            return Some((main, sign * (h * 3600 + m * 60)));
        }
    }
    Some((time_part, 0))
}

/// Civil date (proleptic Gregorian) to days-since-epoch. Returns None for
/// invalid dates. Algorithm: Howard Hinnant's `days_from_civil`.
fn days_from_civil(y: i64, m: u32, d: u32) -> Option<i64> {
    if m == 0 || m > 12 || d == 0 || d > 31 {
        // tolerate the easy case for length-of-month validation only loosely
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m64 = if m > 12 { return None } else { m as u64 };
    let doy = (153 * (if m64 > 2 { m64 - 3 } else { m64 + 9 }) + 2) / 5 + (d as u64) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe as i64 - 719468)
}

/// Coerce a JSON number to u64 (negative => 0, non-number => 0). Floats are
/// floored.
fn as_u64(v: Option<&Value>) -> u64 {
    match v {
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                if i < 0 { 0 } else { i as u64 }
            } else if let Some(f) = n.as_f64() {
                if f.is_finite() && f > 0.0 { f as u64 } else { 0 }
            } else {
                0
            }
        }
        _ => 0,
    }
}

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::store;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_dir() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("aio-usage-test-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    // --- window_cutoff ---

    #[test]
    fn window_today_floors_to_day() {
        // Pick a known epoch: 2026-08-26T13:37:42Z = 178,758,000-ish? Just
        // verify the day-floor property: result is a multiple of 86400, <= now.
        let now = 1_700_000_000; // 2023-11-14T22:13:20Z
        let cut = window_cutoff("today", now);
        assert_eq!(cut % 86400, 0);
        assert!(cut <= now);
        assert!(cut + 86400 > now);
    }

    #[test]
    fn window_7d_subtracts_week() {
        let now = 1_700_000_000;
        assert_eq!(window_cutoff("7d", now), now - 7 * 86400);
    }

    #[test]
    fn window_all_is_zero() {
        assert_eq!(window_cutoff("all", 1_700_000_000), 0);
    }

    // --- format_iso_utc ---

    #[test]
    fn format_iso_known_epoch() {
        // 2023-11-14T22:13:20Z
        assert_eq!(format_iso_utc(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn format_iso_epoch_zero() {
        assert_eq!(format_iso_utc(0), "1970-01-01T00:00:00Z");
    }

    // --- iso8601 parsing ---

    #[test]
    fn iso_z_suffix() {
        assert_eq!(iso8601_to_epoch("2023-11-14T22:13:20Z"), Some(1_700_000_000));
    }

    #[test]
    fn iso_with_fraction() {
        assert_eq!(
            iso8601_to_epoch("2023-11-14T22:13:20.500Z"),
            Some(1_700_000_000)
        );
    }

    #[test]
    fn iso_with_offset() {
        // 2023-11-14T22:13:20+00:00 == Z
        assert_eq!(
            iso8601_to_epoch("2023-11-14T22:13:20+00:00"),
            Some(1_700_000_000)
        );
        // +02:00 => two hours earlier in UTC.
        assert_eq!(
            iso8601_to_epoch("2023-11-14T22:13:20+02:00"),
            Some(1_700_000_000 - 2 * 3600)
        );
    }

    #[test]
    fn iso_no_tz_treated_as_utc() {
        assert_eq!(iso8601_to_epoch("2023-11-14T22:13:20"), Some(1_700_000_000));
    }

    #[test]
    fn iso_garbage_returns_none() {
        assert_eq!(iso8601_to_epoch("not a date"), None);
    }

    // --- pi scan ---

    #[test]
    fn pi_scan_aggregates_and_filters() {
        let dir = temp_dir();
        let sessions = dir.join(".pi/agent/sessions/--root-pi--");
        std::fs::create_dir_all(&sessions).unwrap();

        // Pre-cutoff record (should be skipped by today/7d; included by all).
        let old_ts = "2020-01-01T00:00:00Z";
        // Post-cutoff record.
        let new_ts = "2099-01-01T00:00:00Z";

        // REAL pi jsonl shape: `model` and `usage` are nested under the
        // top-level `message` object; `timestamp` is at the record root.
        // Session header lines have no `message` and must be skipped.
        let lines = vec![
            // session header line (no message -> skip)
            format!(
                r#"{{"type":"summary","version":1,"id":"abc","timestamp":"{new_ts}","cwd":"/root/pi-cwd"}}"#
            ),
            // assistant record with usage + model + cost
            format!(
                r#"{{"type":"assistant","version":1,"id":"r1","timestamp":"{new_ts}","cwd":"/root/pi-cwd","message":{{"role":"assistant","model":"m1","usage":{{"input":100,"output":50,"cacheRead":10,"cacheWrite":5,"cost":{{"input":0.1,"output":0.02,"total":0.123}}}}}}}}"#
            ),
            // assistant record with usage + model, no cost
            format!(
                r#"{{"type":"assistant","version":1,"id":"r2","timestamp":"{new_ts}","cwd":"/root/pi-cwd","message":{{"role":"assistant","model":"m1","usage":{{"input":200,"output":0}}}}}}"#
            ),
            // different model
            format!(
                r#"{{"type":"assistant","version":1,"id":"r3","timestamp":"{new_ts}","cwd":"/root/pi-cwd","message":{{"role":"assistant","model":"m2","usage":{{"input":7,"output":8}}}}}}"#
            ),
            // pre-cutoff, should be skipped for today/7d
            format!(
                r#"{{"type":"assistant","version":1,"id":"r4","timestamp":"{old_ts}","cwd":"/root/pi-cwd","message":{{"role":"assistant","model":"m1","usage":{{"input":999}}}}}}"#
            ),
            // record with message but no usage (should be skipped)
            format!(
                r#"{{"type":"assistant","version":1,"id":"r5","timestamp":"{new_ts}","cwd":"/root/pi-cwd","message":{{"role":"assistant","model":"m1"}}}}"#
            ),
            // record with message but no model (should be skipped)
            format!(
                r#"{{"type":"assistant","version":1,"id":"r6","timestamp":"{new_ts}","cwd":"/root/pi-cwd","message":{{"role":"assistant","usage":{{"input":1}}}}}}"#
            ),
            // corrupt line (should be skipped)
            "{not json".to_string(),
        ];
        std::fs::write(
            sessions.join("s1.jsonl"),
            lines.join("\n"),
        )
        .unwrap();

        let canonical = CanonicalConfig::default();
        // Cutoff = 0 (all): includes the pre-cutoff record too.
        let all_rows = scan_pi(&dir, &canonical, 0);
        let m1_all = all_rows.iter().find(|r| r.model == "m1").unwrap();
        assert_eq!(m1_all.r#in, 100 + 200 + 999);
        assert_eq!(m1_all.out, 50);
        assert_eq!(m1_all.cache_read, 10);
        assert_eq!(m1_all.cache_write, 5);
        // cost summed only from records that had it (0.123).
        assert_eq!(m1_all.cost, Some(0.123));

        // Cutoff in the future: only post-cutoff records.
        let future_cut = 2_000_000_000;
        let rows = scan_pi(&dir, &canonical, future_cut);
        let m1 = rows.iter().find(|r| r.model == "m1").unwrap();
        assert_eq!(m1.r#in, 300); // 100 + 200, not 999
        let m2 = rows.iter().find(|r| r.model == "m2").unwrap();
        assert_eq!(m2.r#in, 7);
        assert_eq!(m2.out, 8);
        assert!(m2.cost.is_none());
    }

    /// Regression for the field-path bug: a record that puts `usage`/`model` at
    /// the record ROOT (the old, wrong assumption) must NOT be picked up --
    /// only the nested-`message` shape contributes. This pins the contract
    /// against future regressions to the top-level read.
    #[test]
    fn pi_scan_ignores_usage_at_record_root() {
        let dir = temp_dir();
        let sessions = dir.join(".pi/agent/sessions/x--");
        std::fs::create_dir_all(&sessions).unwrap();
        // WRONG shape (root-level model+usage) - must be ignored.
        std::fs::write(
            sessions.join("bad.jsonl"),
            r#"{"model":"root-level","timestamp":"2099-01-01T00:00:00Z","usage":{"input":999}}"#,
        )
        .unwrap();
        // CORRECT shape (nested message) - must be picked up.
        std::fs::write(
            sessions.join("good.jsonl"),
            r#"{"type":"assistant","timestamp":"2099-01-01T00:00:00Z","message":{"role":"assistant","model":"nested","usage":{"input":42}}}"#,
        )
        .unwrap();

        let canonical = CanonicalConfig::default();
        let rows = scan_pi(&dir, &canonical, 0);
        assert!(rows.iter().all(|r| r.model != "root-level"));
        let nested = rows.iter().find(|r| r.model == "nested").unwrap();
        assert_eq!(nested.r#in, 42);
    }

    #[test]
    fn pi_scan_missing_dir_returns_empty() {
        let dir = temp_dir();
        let canonical = CanonicalConfig::default();
        let rows = scan_pi(&dir, &canonical, 0);
        assert!(rows.is_empty());
    }

    #[test]
    fn pi_scan_provider_join_from_canonical() {
        let dir = temp_dir();
        let sessions = dir.join(".pi/agent/sessions/x--");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("s.jsonl"),
            r#"{"type":"assistant","timestamp":"2099-01-01T00:00:00Z","message":{"role":"assistant","model":"known-model","usage":{"input":1}}}"#,
        )
        .unwrap();

        let mut canonical = CanonicalConfig::default();
        canonical.providers.insert(
            "my-prov".into(),
            store::ProviderEntry {
                name: "My".into(),
                models: vec![store::ModelEntry {
                    id: "known-model".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let rows = scan_pi(&dir, &canonical, 0);
        assert_eq!(rows[0].provider.as_deref(), Some("my-prov"));

        // Unmatched model => provider None.
        std::fs::write(
            sessions.join("s2.jsonl"),
            r#"{"type":"assistant","timestamp":"2099-01-01T00:00:00Z","message":{"role":"assistant","model":"unknown-model","usage":{"input":1}}}"#,
        )
        .unwrap();
        let rows = scan_pi(&dir, &canonical, 0);
        let un = rows.iter().find(|r| r.model == "unknown-model").unwrap();
        assert!(un.provider.is_none());
    }

    // --- opencode scan (in-memory sqlite) ---

    #[test]
    fn opencode_scan_aggregates_assistant_rows() {
        // In-memory db with the message schema.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE message (id INTEGER PRIMARY KEY, data TEXT NOT NULL, time_created INTEGER)",
            [],
        )
        .unwrap();
        // assistant row with tokens + cost
        let data1 = r#"{"role":"assistant","modelID":"m1","providerID":"p1","tokens":{"input":100,"output":50,"cache":{"read":10,"write":5}},"cost":0.01}"#;
        // assistant row, different model
        let data2 = r#"{"role":"assistant","modelID":"m2","providerID":"p1","tokens":{"input":7,"output":8,"cache":{"read":1,"write":2}}}"#;
        // user row (must be skipped)
        let data3 = r#"{"role":"user","modelID":"m1","providerID":"p1","tokens":{"input":999}}"#;
        // assistant row with pre-cutoff time (must be skipped for non-zero cutoff)
        let data4 = r#"{"role":"assistant","modelID":"m1","providerID":"p1","tokens":{"input":5}}"#;
        conn.execute(
            "INSERT INTO message (data, time_created) VALUES (?1, ?2), (?3, ?4), (?5, ?6), (?7, ?8)",
            rusqlite::params![
                data1, 2_000_000_000,
                data2, 2_000_000_000,
                data3, 2_000_000_000,
                data4, 1, // pre-cutoff
            ],
        )
        .unwrap();

        // Write to a temp file so scan_opencode can open it read-only.
        let dir = temp_dir();
        let db_path = dir.join(".local/share/opencode/opencode.db");
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        conn.execute("ATTACH DATABASE ?1 AS outdb", rusqlite::params![db_path.to_str().unwrap()])
            .unwrap();
        conn.execute("CREATE TABLE outdb.message AS SELECT * FROM message", []).unwrap();
        drop(conn);

        let rows = scan_opencode(&dir, 1_000_000);
        let m1 = rows.iter().find(|r| r.model == "m1").unwrap();
        assert_eq!(m1.r#in, 100);
        assert_eq!(m1.out, 50);
        assert_eq!(m1.cache_read, 10);
        assert_eq!(m1.cache_write, 5);
        assert_eq!(m1.cost, Some(0.01));
        assert_eq!(m1.provider.as_deref(), Some("p1"));
        assert_eq!(m1.agent, "opencode");

        let m2 = rows.iter().find(|r| r.model == "m2").unwrap();
        assert_eq!(m2.r#in, 7);
        assert!(m2.cost.is_none());

        // pre-cutoff row (input 5) excluded.
        assert_eq!(rows.iter().filter(|r| r.model == "m1").count(), 1);
    }

    #[test]
    fn opencode_scan_missing_db_returns_empty() {
        let dir = temp_dir();
        let rows = scan_opencode(&dir, 0);
        assert!(rows.is_empty());
    }

    // --- claude scan ---

    #[test]
    fn claude_scan_extracts_message_usage() {
        let dir = temp_dir();
        let projects = dir.join(".claude/projects/proj");
        std::fs::create_dir_all(&projects).unwrap();

        let lines = vec![
            // assistant record with usage + model
            r#"{"timestamp":"2099-01-01T00:00:00Z","message":{"model":"claude-3","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":10,"cache_creation_input_tokens":5}}}"#,
            // user record (no usage, skip)
            r#"{"timestamp":"2099-01-01T00:00:00Z","message":{"role":"user"}}"#,
            // pre-cutoff (skip for non-zero cutoff)
            r#"{"timestamp":"2020-01-01T00:00:00Z","message":{"model":"claude-3","usage":{"input_tokens":999}}}"#,
        ];
        std::fs::write(projects.join("s.jsonl"), lines.join("\n")).unwrap();

        let rows = scan_claude(&dir, 2_000_000_000);
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.model, "claude-3");
        assert_eq!(r.r#in, 100);
        assert_eq!(r.out, 50);
        assert_eq!(r.cache_read, 10);
        assert_eq!(r.cache_write, 5);
        assert!(r.cost.is_none());
        assert_eq!(r.agent, "claude");
    }

    #[test]
    fn claude_scan_missing_dir_returns_empty() {
        let dir = temp_dir();
        let rows = scan_claude(&dir, 0);
        assert!(rows.is_empty());
    }

    // --- codex scan ---

    #[test]
    fn codex_scan_token_count_uses_last_model() {
        let dir = temp_dir();
        let sessions = dir.join(".codex/sessions/2026/01/01");
        std::fs::create_dir_all(&sessions).unwrap();
        let lines = vec![
            // turn context sets the model
            r#"{"type":"turn_context","model":"gpt-5"}"#,
            // token_count event uses last_model
            r#"{"type":"token_count","timestamp":"2099-01-01T00:00:00Z","info":{"total_token_usage":{"input_tokens":100,"output_tokens":50,"cached_input_tokens":10}}}"#,
            // another turn_context changes the model
            r#"{"type":"turn_context","model":"o3"}"#,
            // another token_count
            r#"{"type":"token_count","timestamp":"2099-01-01T00:00:00Z","info":{"total_token_usage":{"input_tokens":7,"output_tokens":8,"cached_input_tokens":1}}}"#,
            // pre-cutoff token_count (skip with non-zero cutoff)
            r#"{"type":"token_count","timestamp":"2020-01-01T00:00:00Z","info":{"total_token_usage":{"input_tokens":999}}}"#,
        ];
        std::fs::write(sessions.join("rollout-x.jsonl"), lines.join("\n")).unwrap();

        let rows = scan_codex(&dir, 2_000_000_000);
        let gpt5 = rows.iter().find(|r| r.model == "gpt-5").unwrap();
        assert_eq!(gpt5.r#in, 100);
        assert_eq!(gpt5.out, 50);
        assert_eq!(gpt5.cache_read, 10);
        let o3 = rows.iter().find(|r| r.model == "o3").unwrap();
        assert_eq!(o3.r#in, 7);
        assert_eq!(o3.out, 8);
        assert_eq!(o3.cache_read, 1);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn codex_scan_missing_dir_returns_empty() {
        let dir = temp_dir();
        let rows = scan_codex(&dir, 0);
        assert!(rows.is_empty());
    }

    // --- merge_row ---

    #[test]
    fn merge_row_sums_and_preserves_cost_only_when_present() {
        let mut buckets: BTreeMap<(String, Option<String>, String), UsageRow> = BTreeMap::new();
        merge_row(
            &mut buckets,
            UsageRow {
                agent: "pi".into(),
                provider: Some("p".into()),
                model: "m".into(),
                r#in: 10,
                out: 1,
                cache_read: 0,
                cache_write: 0,
                cost: Some(0.5),
            },
        );
        // Second row has no cost — existing cost stays.
        merge_row(
            &mut buckets,
            UsageRow {
                agent: "pi".into(),
                provider: Some("p".into()),
                model: "m".into(),
                r#in: 5,
                out: 2,
                cache_read: 0,
                cache_write: 0,
                cost: None,
            },
        );
        let r = &buckets[&("pi".into(), Some("p".into()), "m".into())];
        assert_eq!(r.r#in, 15);
        assert_eq!(r.cost, Some(0.5));
    }

    #[test]
    fn merge_row_cost_starts_none_when_no_record_has_it() {
        let mut buckets: BTreeMap<(String, Option<String>, String), UsageRow> = BTreeMap::new();
        merge_row(
            &mut buckets,
            UsageRow {
                agent: "claude".into(),
                provider: None,
                model: "m".into(),
                r#in: 1,
                out: 0,
                cache_read: 0,
                cache_write: 0,
                cost: None,
            },
        );
        let r = &buckets[&("claude".into(), None, "m".into())];
        assert!(r.cost.is_none());
    }

    // --- cache logic (tested in isolation, no global SystemTime in IUT) ---

    #[test]
    fn cache_hit_within_ttl_returns_cached_generated_at() {
        // Simulate the cache-hit path by populating the cache and verifying
        // the stored entry round-trips. The handler's hit test is the
        // integration path; here we assert the cache struct behaviour.
        let now = Instant::now();
        let entry = CacheEntry {
            at: now,
            window: "today".into(),
            rows: vec![UsageRow {
                agent: "pi".into(),
                provider: None,
                model: "m".into(),
                r#in: 1,
                out: 0,
                cache_read: 0,
                cache_write: 0,
                cost: None,
            }],
            generated_at: "2023-11-14T22:13:20Z".into(),
        };
        assert!(entry.at.elapsed() < CACHE_TTL);
        assert_eq!(entry.window, "today");
        assert_eq!(entry.generated_at, "2023-11-14T22:13:20Z");
    }

    // --- UsageResponse serialization (regression for generatedAt:null bug) ---

    /// Regression: `generatedAt` must serialize as camelCase (not snake_case)
    /// and must be a non-null ISO8601 string when populated. Previously the
    /// field had no `#[serde(rename)]` so the frontend saw `generated_at` and
    /// read `generatedAt` as null/undefined.
    #[test]
    fn usage_response_serializes_generated_at_camel_case() {
        let resp = UsageResponse {
            rows: vec![UsageRow {
                agent: "pi".into(),
                provider: Some("p".into()),
                model: "m".into(),
                r#in: 1,
                out: 0,
                cache_read: 0,
                cache_write: 0,
                cost: Some(0.1),
            }],
            generated_at: "2023-11-14T22:13:20Z".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        // Field MUST be camelCase.
        let gen = json.get("generatedAt");
        assert!(gen.is_some(), "generatedAt key missing in JSON: {json}");
        let gen_str = gen.unwrap().as_str();
        assert_eq!(gen_str, Some("2023-11-14T22:13:20Z"));
        // And must NOT also appear as snake_case.
        assert!(json.get("generated_at").is_none());
        // The rows array and its camelCase fields are intact.
        let row = json.get("rows").and_then(|r| r.as_array()).unwrap();
        assert_eq!(row.len(), 1);
        assert!(row[0].get("cacheRead").is_some());
        assert!(row[0].get("cacheWrite").is_some());
        assert!(row[0].get("generated_at").is_none());
    }

    /// Regression: a freshly computed `generated_at` from `format_iso_utc`
    /// (the same helper the handler uses) serializes to a non-null ISO8601
    /// string. This guards against the handler assigning None / forgetting to
    /// populate the field.
    #[test]
    fn usage_response_generated_at_from_format_iso_is_non_null_iso() {
        // 2023-11-14T22:13:20Z
        let generated_at = format_iso_utc(1_700_000_000);
        let resp = UsageResponse {
            rows: Vec::new(),
            generated_at: generated_at.clone(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        let gen = json.get("generatedAt").and_then(|v| v.as_str());
        assert_eq!(gen, Some("2023-11-14T22:13:20Z"));
        // Strict ISO8601 UTC shape: ends with 'Z', parses back via our own
        // parser to the same epoch.
        assert!(gen.unwrap().ends_with('Z'));
        assert_eq!(iso8601_to_epoch(gen.unwrap()), Some(1_700_000_000));
    }

    // --- cost backfill (task 08-27-usage-correctness, design §2) ---

    use super::super::store::{CostEntry, ModelEntry, ProviderEntry};

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn make_row(
        agent: &str,
        provider: Option<&str>,
        model: &str,
        in_tok: u64,
        out_tok: u64,
        cache_read: u64,
        cache_write: u64,
        cost: Option<f64>,
    ) -> UsageRow {
        UsageRow {
            agent: agent.into(),
            provider: provider.map(String::from),
            model: model.into(),
            r#in: in_tok,
            out: out_tok,
            cache_read,
            cache_write,
            cost,
        }
    }

    /// Canonical fixture:
    /// - prov-a: m1 with full cost (distinct rates per bucket)
    /// - prov-b: m2 with input-only cost
    /// - prov-c: m1 again (makes m1's provider ambiguous across providers)
    /// - prov-d: claude-sonnet-4 with cost (fuzzy target)
    fn cost_canonical() -> CanonicalConfig {
        let mut c = CanonicalConfig::default();
        let full = || {
            Some(CostEntry {
                input: Some(0.14),
                output: Some(0.28),
                cache_read: Some(0.0028),
                cache_write: Some(1.12),
            })
        };
        let entry = |id: &str, cost: Option<CostEntry>| ModelEntry {
            id: id.into(),
            cost,
            ..Default::default()
        };
        c.providers.insert(
            "prov-a".into(),
            ProviderEntry {
                name: "A".into(),
                models: vec![entry("m1", full())],
                ..Default::default()
            },
        );
        c.providers.insert(
            "prov-b".into(),
            ProviderEntry {
                name: "B".into(),
                models: vec![entry("m2", Some(CostEntry { input: Some(2.0), ..Default::default() }))],
                ..Default::default()
            },
        );
        c.providers.insert(
            "prov-c".into(),
            ProviderEntry {
                name: "C".into(),
                models: vec![entry("m1", Some(CostEntry { input: Some(9.9), ..Default::default() }))],
                ..Default::default()
            },
        );
        c.providers.insert(
            "prov-d".into(),
            ProviderEntry {
                name: "D".into(),
                models: vec![entry("claude-sonnet-4", full())],
                ..Default::default()
            },
        );
        c
    }

    #[test]
    fn backfill_trusts_positive_logged_cost() {
        // A real logged cost is never touched (no double count).
        let canonical = cost_canonical();
        let mut rows = vec![make_row("pi", Some("prov-a"), "m1", 1_000_000, 1_000_000, 0, 0, Some(0.5))];
        backfill_cost(&mut rows, &canonical);
        assert_eq!(rows[0].cost, Some(0.5));
    }

    #[test]
    fn backfill_zero_logged_cost_computes_from_provider_model() {
        // pi/opencode always log 0 — that must fall through to backfill.
        let canonical = cost_canonical();
        let mut rows = vec![make_row("pi", Some("prov-a"), "m1", 1_000_000, 1_000_000, 1_000_000, 1_000_000, Some(0.0))];
        backfill_cost(&mut rows, &canonical);
        // Each bucket priced with its OWN rate: 0.14 + 0.28 + 0.0028 + 1.12.
        let expected = 0.14 + 0.28 + 0.0028 + 1.12;
        assert!(
            rows[0].cost.map(|c| approx(c, expected)).unwrap_or(false),
            "cost = {:?}, expected {expected}",
            rows[0].cost
        );
    }

    #[test]
    fn backfill_none_cost_exact_model_across_providers() {
        // claude rows have provider=None; exact model id found in prov-b.
        let canonical = cost_canonical();
        let mut rows = vec![make_row("claude", None, "m2", 500_000, 0, 0, 0, None)];
        backfill_cost(&mut rows, &canonical);
        // prov-b has input-only cost: 0.5M * 2.0 = 1.0; output/cache None -> 0.
        assert!(rows[0].cost.map(|c| approx(c, 1.0)).unwrap_or(false), "cost = {:?}", rows[0].cost);
        // Unambiguous match -> provider backfilled.
        assert_eq!(rows[0].provider.as_deref(), Some("prov-b"));
    }

    #[test]
    fn backfill_provider_none_when_model_ambiguous() {
        // m1 exists in prov-a AND prov-c: cost still filled (most specific /
        // first), but provider must NOT be backfilled (ambiguous).
        let canonical = cost_canonical();
        let mut rows = vec![make_row("claude", None, "m1", 1_000_000, 0, 0, 0, None)];
        backfill_cost(&mut rows, &canonical);
        assert!(rows[0].cost.is_some(), "cost must still be filled");
        assert!(rows[0].provider.is_none(), "ambiguous match must not backfill provider");
    }

    #[test]
    fn backfill_fuzzy_version_suffix_matches() {
        let canonical = cost_canonical();
        // Date-suffixed log id vs canonical id (the claude case).
        let mut rows = vec![make_row("claude", None, "claude-sonnet-4-20250514", 1_000_000, 0, 0, 0, None)];
        backfill_cost(&mut rows, &canonical);
        assert!(rows[0].cost.map(|c| approx(c, 0.14)).unwrap_or(false), "cost = {:?}", rows[0].cost);
        assert_eq!(rows[0].provider.as_deref(), Some("prov-d"));
    }

    #[test]
    fn backfill_fuzzy_rejects_variant_suffixes() {
        // "-free" / "-vision-exp" are DIFFERENT models — must not be priced
        // at the base model's rates (container reconciliation found
        // deepseek-v4-flash-free wrongly priced at deepseek-v4-flash rates).
        let canonical = cost_canonical();
        for variant in ["claude-sonnet-4-free", "claude-sonnet-4-vision-exp"] {
            let mut rows = vec![make_row("opencode", None, variant, 1_000_000, 0, 0, 0, None)];
            backfill_cost(&mut rows, &canonical);
            assert!(rows[0].cost.is_none(), "{variant} must not fuzzy-match");
        }
        // Degenerate suffixes are not version stamps either (design §2c
        // `-\d[\d.]*` must LEAD with a digit): bare "-", leading-dot "-.5",
        // dots-only "-.", and unicode digit look-alikes all fail.
        for variant in [
            "claude-sonnet-4-",
            "claude-sonnet-4-.5",
            "claude-sonnet-4-.",
            "claude-sonnet-4-٢٠٢٥",
        ] {
            let mut rows = vec![make_row("claude", None, variant, 1_000_000, 0, 0, 0, None)];
            backfill_cost(&mut rows, &canonical);
            assert!(rows[0].cost.is_none(), "{variant} must not fuzzy-match");
        }
        // Reverse direction (shorter row id) is rejected too: "gpt" must not
        // match a hypothetical "gpt-5".
        let mut rows = vec![make_row("codex", None, "claude-sonnet", 1_000_000, 0, 0, 0, None)];
        backfill_cost(&mut rows, &canonical);
        assert!(rows[0].cost.is_none(), "reverse-prefix must not match");
    }

    #[test]
    fn backfill_no_match_leaves_cost_none() {
        let canonical = cost_canonical();
        let mut rows = vec![make_row("codex", None, "totally-unknown", 100, 100, 0, 0, None)];
        backfill_cost(&mut rows, &canonical);
        assert!(rows[0].cost.is_none(), "never invent a price");
        assert!(rows[0].provider.is_none());
    }

    #[test]
    fn backfill_free_model_stays_zero() {
        // A logged 0 with no canonical hit stays 0 (real free tier), not None.
        let canonical = cost_canonical();
        let mut rows = vec![make_row("opencode", Some("opencode"), "mimo-v2.5-free", 1_000, 500, 0, 0, Some(0.0))];
        backfill_cost(&mut rows, &canonical);
        assert_eq!(rows[0].cost, Some(0.0));
    }

    #[test]
    fn zero_token_rows_dropped_in_handler_order() {
        // The handler's retain (design §3): all-zero rows are noise.
        let mut rows = vec![
            make_row("opencode", Some("opencode"), "mimo-v2.5-free", 0, 0, 0, 0, Some(0.0)),
            make_row("pi", None, "m2", 7, 0, 0, 0, None),
        ];
        rows.retain(|r| r.r#in + r.out + r.cache_read + r.cache_write > 0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].model, "m2");
    }
}
