// SQLite state.db (sandbox-mgr Phase 1, design §3.3).
//
// Schema: sandboxes / images / jobs / kv. kv is created now but unused until
// Phase 4 (models_config canonical store) - one migration-free schema from
// the start beats ALTER TABLE churn later. All helpers lock the connection
// briefly (state.rs module comment) and return owned data.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::state::JobShared;

/// Open (creating parent dirs) and initialize state.db. Idempotent - every
/// CREATE is IF NOT EXISTS - so mgr restarts are safe.
pub fn open(db_path: &Path) -> Result<Connection> {
    if let Some(dir) = db_path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("create {}", dir.display()))?;
    }
    let conn = Connection::open(db_path)
        .with_context(|| format!("open {}", db_path.display()))?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sandboxes (
  name TEXT PRIMARY KEY,            -- user slug, doubles as subdomain prefix
  created_at INTEGER NOT NULL,
  env_json TEXT NOT NULL,           -- SandboxEnv {scenarios, versions}
  env_hash TEXT NOT NULL,           -- sha256 of the assembled Dockerfile.base
  cpus REAL,
  mem_mb INTEGER,
  status TEXT NOT NULL,             -- running|stopped|creating|error
  adopted INTEGER DEFAULT 0,        -- 1 = imported external stack (Phase 5)
  external_compose TEXT
);
CREATE TABLE IF NOT EXISTS images (
  env_hash TEXT PRIMARY KEY,
  tag TEXT NOT NULL,                -- sandbox-base-<env_hash[:12]>
  built_at INTEGER,
  build_log TEXT                    -- last build output tail (failure display)
);
CREATE TABLE IF NOT EXISTS jobs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL,
  sandbox TEXT,
  status TEXT NOT NULL,
  error TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT);
";

/// One sandboxes row (design §3.3). env_json is parsed by the caller.
#[derive(Debug, Clone)]
pub struct SandboxRow {
    pub name: String,
    pub created_at: i64,
    pub env_json: String,
    pub env_hash: String,
    pub cpus: Option<f64>,
    pub mem_mb: Option<i64>,
    pub status: String,
    pub adopted: bool,
    pub external_compose: Option<String>,
}

fn row_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<SandboxRow> {
    Ok(SandboxRow {
        name: row.get(0)?,
        created_at: row.get(1)?,
        env_json: row.get(2)?,
        env_hash: row.get(3)?,
        cpus: row.get(4)?,
        mem_mb: row.get(5)?,
        status: row.get(6)?,
        adopted: row.get::<_, i64>(7)? != 0,
        external_compose: row.get(8)?,
    })
}

const SANDBOX_COLS: &str =
    "name, created_at, env_json, env_hash, cpus, mem_mb, status, adopted, external_compose";

pub fn insert_sandbox(conn: &Connection, row: &SandboxRow) -> Result<()> {
    conn.execute(
        &format!("INSERT INTO sandboxes ({SANDBOX_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"),
        params![
            row.name,
            row.created_at,
            row.env_json,
            row.env_hash,
            row.cpus,
            row.mem_mb,
            row.status,
            row.adopted as i64,
            row.external_compose,
        ],
    )?;
    Ok(())
}

pub fn get_sandbox(conn: &Connection, name: &str) -> Result<Option<SandboxRow>> {
    let sql = format!("SELECT {SANDBOX_COLS} FROM sandboxes WHERE name = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![name])?;
    match rows.next()? {
        Some(r) => Ok(Some(row_from(r)?)),
        None => Ok(None),
    }
}

pub fn list_sandboxes(conn: &Connection) -> Result<Vec<SandboxRow>> {
    let sql = format!("SELECT {SANDBOX_COLS} FROM sandboxes ORDER BY name");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_from)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn update_sandbox_status(conn: &Connection, name: &str, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE sandboxes SET status = ?2 WHERE name = ?1",
        params![name, status],
    )?;
    Ok(())
}

/// PUT /api/sandboxes/:name after a successful (re)create: env and resource
/// config move together with the new hash.
pub fn update_sandbox_config(
    conn: &Connection,
    name: &str,
    env_json: &str,
    env_hash: &str,
    cpus: Option<f64>,
    mem_mb: Option<i64>,
) -> Result<()> {
    conn.execute(
        "UPDATE sandboxes SET env_json = ?2, env_hash = ?3, cpus = ?4, mem_mb = ?5 WHERE name = ?1",
        params![name, env_json, env_hash, cpus, mem_mb],
    )?;
    Ok(())
}

pub fn delete_sandbox(conn: &Connection, name: &str) -> Result<()> {
    conn.execute("DELETE FROM sandboxes WHERE name = ?1", params![name])?;
    Ok(())
}

/// Record an image row after a create job. `build_log` empty means "the image
/// already existed and nothing was built" (A5 same-env reuse - jobs.rs still
/// upserts so a row exists for an image whose record predates the DB): in
/// that case the original build's built_at/build_log MUST survive; a real
/// rebuild (non-empty log) replaces both.
pub fn upsert_image(
    conn: &Connection,
    env_hash: &str,
    tag: &str,
    build_log: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO images (env_hash, tag, built_at, build_log) VALUES (?1,?2,?3,?4)
         ON CONFLICT(env_hash) DO UPDATE SET
           built_at = CASE WHEN ?4 = '' THEN images.built_at ELSE ?3 END,
           build_log = CASE WHEN ?4 = '' THEN images.build_log ELSE ?4 END",
        params![
            env_hash,
            tag,
            chrono_now_secs(),
            build_log,
        ],
    )?;
    Ok(())
}

pub fn list_images(conn: &Connection) -> Result<Vec<(String, String, i64, String)>> {
    let mut stmt = conn.prepare("SELECT env_hash, tag, built_at, build_log FROM images ORDER BY built_at DESC")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

/// How many sandboxes currently reference this env (design §3.3: no separate
/// refcount table - just COUNT(*)).
pub fn image_refcount(conn: &Connection, env_hash: &str) -> Result<i64> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM sandboxes WHERE env_hash = ?1",
        params![env_hash],
        |r| r.get(0),
    )?;
    Ok(n)
}

pub fn insert_job(conn: &Connection, kind: &str, sandbox: Option<&str>) -> Result<i64> {
    let now = chrono_now_secs();
    conn.execute(
        "INSERT INTO jobs (kind, sandbox, status, created_at, updated_at) VALUES (?1,?2,'running',?3,?3)",
        params![kind, sandbox, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn persist_job(conn: &Connection, job: &JobShared) -> Result<()> {
    conn.execute(
        "UPDATE jobs SET status = ?2, error = ?3, updated_at = ?4 WHERE id = ?1",
        params![job.id, job.status, job.error, chrono_now_secs()],
    )?;
    Ok(())
}

/// On startup: any job left `running` by a previous mgr process is dead -
/// mark it error so the poller UI doesn't show a phantom build forever.
pub fn fail_orphan_jobs(conn: &Connection) -> Result<usize> {
    let n = conn.execute(
        "UPDATE jobs SET status = 'error', error = 'interrupted (mgr restarted)',
         updated_at = ?1 WHERE status = 'running'",
        params![chrono_now_secs()],
    )?;
    Ok(n)
}

// ── kv ─────────────────────────────────────────────────────────────

/// Set a kv row (upsert). Currently: `gateway_reload` = outcome of the last
/// total-gateway regeneration ("ok" or the reload error text, caddy.rs
/// Phase 2; Phase 4 adds models_config). Keyed by literal call sites - no
/// generic registry needed at this scale.
pub fn kv_set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO kv (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2",
        params![key, value],
    )?;
    Ok(())
}

/// Seconds since epoch. (mgr has no chrono dep; std::time is enough.)
fn chrono_now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(SCHEMA).expect("init schema");
        conn
    }

    #[test]
    fn upsert_image_empty_log_keeps_existing_record() {
        // A5 same-env reuse: the second create upserts with "" (nothing was
        // built); the original build's built_at + log must survive, or the
        // images page loses its build log the first time a config is reused.
        let conn = mem_db();
        upsert_image(&conn, "h1", "sandbox-base-h1", "original log").unwrap();
        upsert_image(&conn, "h1", "sandbox-base-h1", "").unwrap();
        let (built_at, build_log) = conn
            .query_row(
                "SELECT built_at, build_log FROM images WHERE env_hash = 'h1'",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .unwrap();
        assert_eq!(build_log, "original log");
        assert!(built_at > 0);
    }

    #[test]
    fn upsert_image_real_rebuild_replaces_log() {
        let conn = mem_db();
        upsert_image(&conn, "h2", "sandbox-base-h2", "old").unwrap();
        upsert_image(&conn, "h2", "sandbox-base-h2", "new log").unwrap();
        let build_log: String = conn
            .query_row("SELECT build_log FROM images WHERE env_hash = 'h2'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(build_log, "new log");
    }

    #[test]
    fn upsert_image_first_insert_with_empty_log_stores_row() {
        // No prior row: the empty-log preservation branch must not swallow
        // the INSERT (CASE only fires on conflict).
        let conn = mem_db();
        upsert_image(&conn, "h3", "sandbox-base-h3", "").unwrap();
        let (tag, build_log): (String, String) = conn
            .query_row("SELECT tag, build_log FROM images WHERE env_hash = 'h3'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(tag, "sandbox-base-h3");
        assert_eq!(build_log, "");
    }
}
