// Shared helpers for the per-agent renderers (design §4 common flow).
//
// Every renderer follows: read canonical -> resolve agent assignment ->
// backup target file (rolling .aio-bak-<ISO> newest 3) -> merge render ->
// atomic write (0600 for secret-bearing files, 0644 for the rest) ->
// per-file status reported via ApplyResult. A corrupt target file aborts
// THAT file's write but does not stop the other files of the same renderer
// (pi has two files; codex rolls back auth.json when config.toml fails).

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

// ── result type ───────────────────────────────────────────────────

/// Per-render apply outcome. `ok` is false as soon as any file errors.
/// `written`/`errors` are reported per-file so the UI can show exactly
/// which files changed and which failed (design §3 /api/models/apply).
#[derive(Debug, Default, Clone, Serialize)]
pub struct ApplyResult {
    pub ok: bool,
    pub written: Vec<FileWritten>,
    pub errors: Vec<FileError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileWritten {
    pub path: String,
    /// Backup path if a prior file was backed up before write, else null.
    pub backup: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileError {
    pub path: String,
    pub message: String,
}

impl ApplyResult {
    pub fn new() -> Self {
        Self {
            ok: true,
            written: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn push_ok(&mut self, path: PathBuf, backup: Option<String>) {
        self.written.push(FileWritten {
            path: path.display().to_string(),
            backup,
        });
    }

    pub fn push_err(&mut self, path: PathBuf, message: String) {
        self.ok = false;
        self.errors.push(FileError {
            path: path.display().to_string(),
            message,
        });
    }

    /// Drop every `written` entry whose path string equals `path` (used by
    /// the codex rollback path, which reverts a file written earlier in the
    /// same apply). Returns whether any entry was removed.
    pub fn remove_written(&mut self, path: &str) -> bool {
        let before = self.written.len();
        self.written.retain(|w| w.path != path);
        before != self.written.len()
    }
}

// ── read errors ───────────────────────────────────────────────────

/// Why a target file couldn't be read for merge. Missing files are NOT an
/// error (`read_json_object` returns `Ok(None)` so the renderer creates a
/// fresh object); this is only for corrupt/IO-error cases.
#[derive(Debug)]
pub enum ReadError {
    Corrupt(String),
    Io(std::io::Error),
}

/// Read a JSON object file. Missing file or empty content => `Ok(None)`
/// (renderer creates a fresh object). Corrupt JSON => `Err(Corrupt)`;
/// the renderer must NOT overwrite a corrupt file (design §4).
pub fn read_json_object(path: &Path) -> Result<Option<Value>, ReadError> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(None),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(v) if v.is_object() => Ok(Some(v)),
            Ok(v) => Err(ReadError::Corrupt(format!(
                "expected JSON object, got {}",
                v_type(&v)
            ))),
            Err(e) => Err(ReadError::Corrupt(e.to_string())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ReadError::Io(e)),
    }
}

fn v_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// ── home resolution ───────────────────────────────────────────────

/// Resolve the agent home directory from `$HOME` (default `/home/gem`).
/// Single owner so routes and renderers use the same notion of home
/// (matches the existing `pi_models_path()` helper in models/mod.rs).
pub fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/home/gem".to_string()))
}

// ── backup ────────────────────────────────────────────────────────

/// Backup a file to `<path>.aio-bak-<UTC YYYYMMDDTHHMMSSmmm>` if it exists,
/// then prune older backups of the same file keeping the newest 3. Returns
/// the backup path (so the apply response can surface it) or `None` when the
/// file did not exist (nothing to back up). A backup failure is surfaced as
/// an io::Error; callers decide whether to abort the file or proceed.
pub fn backup_file(path: &Path) -> std::io::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let ts = format_timestamp_utc();
    let backup = format!("{}.aio-bak-{}", path.display(), ts);
    std::fs::copy(path, &backup)?;
    prune_backups(path);
    Ok(Some(backup))
}

/// Keep the 3 newest `<file>.aio-bak-*` siblings; delete the rest. Suffixes
/// are timestamps (lexicographic sort == chronological for fixed-width
/// YYYYMMDDTHHMMSSmmm), so lexicographic sort of the backup file names is
/// the right ordering (design §4).
fn prune_backups(path: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    let Some(file_name) = path.file_name() else {
        return;
    };
    let prefix = format!("{}.aio-bak-", file_name.to_string_lossy());
    let mut backups: Vec<PathBuf> = match std::fs::read_dir(parent) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().starts_with(&prefix))
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => return,
    };
    if backups.len() <= 3 {
        return;
    }
    // Sort ascending by filename (oldest first). Delete all but the newest 3.
    backups.sort();
    let to_delete = backups.len() - 3;
    for p in backups.iter().take(to_delete) {
        let _ = std::fs::remove_file(p);
    }
}

// ── atomic write ──────────────────────────────────────────────────

/// Atomically write `bytes` to `path` via a temp file in the same dir +
/// rename. The temp file is `<path>.aio-tmp` (single writer: all apply
/// paths hold the process-wide `models_lock`). Parent dirs are created
/// 0755; the temp file is chmod'd to `mode` before rename (0600 for
/// secret-bearing files, 0644 for the rest — design §4). Matches the
/// existing `store::write_config` style.
pub fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o755));
    }

    let tmp = format!("{}.aio-tmp", path.display());
    std::fs::write(&tmp, bytes)?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ── backup + write + verify ───────────────────────────────────────

/// Combined backup + atomic write (design §4 common flow). Returns the
/// backup path (or `None` when the file didn't exist before). On write
/// failure the original file is untouched (atomic_write uses a temp +
/// rename, so a failure leaves the original intact) and the backup is a
/// harmless extra copy that prune will clean up later.
pub fn backup_and_atomic_write(
    path: &Path,
    bytes: &[u8],
    mode: u32,
) -> std::io::Result<Option<String>> {
    let backup = backup_file(path)?;
    atomic_write(path, bytes, mode)?;
    Ok(backup)
}

/// Read back a JSON file and verify it parses. Used by renderers after
/// write to catch truncation/corruption (design §4: "After write, read
/// back and verify it parses - on verify failure, restore from backup").
pub fn read_back_verify_json(path: &Path) -> Result<(), String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<Value>(&text)
            .map(|_| ())
            .map_err(|e| format!("verify: parse failed: {e}")),
        Err(e) => Err(format!("verify: read failed: {e}")),
    }
}

/// Read back a TOML file and verify it parses. Used by the codex renderer
/// after writing config.toml (design §4).
pub fn read_back_verify_toml(path: &Path) -> Result<(), String> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .parse::<toml::Value>()
            .map(|_| ())
            .map_err(|e| format!("verify: parse failed: {e}")),
        Err(e) => Err(format!("verify: read failed: {e}")),
    }
}

/// Restore a file from its backup (or remove it when no backup existed -
/// i.e. the file was newly created). Used when read-back verification
/// fails so the target is returned to its pre-apply state (design §4).
pub fn restore_backup_or_remove(path: &Path, backup: Option<&str>) {
    match backup {
        Some(b) => {
            let _ = std::fs::rename(b, path);
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Backup + atomic write + JSON read-back verify. Returns the backup path
/// on success. On verify failure, restores from backup (or removes a newly
/// created file) and returns an error message. Consolidates the design §4
/// common flow for JSON-based renderers (pi, claude, opencode, codex
/// auth.json).
pub fn backup_write_verify_json(
    path: &Path,
    bytes: &[u8],
    mode: u32,
) -> Result<Option<String>, String> {
    let backup =
        backup_and_atomic_write(path, bytes, mode).map_err(|e| format!("write: {e}"))?;
    if let Err(msg) = read_back_verify_json(path) {
        restore_backup_or_remove(path, backup.as_deref());
        return Err(format!("verify failed, restored: {msg}"));
    }
    Ok(backup)
}

/// Backup + atomic write + TOML read-back verify. Same semantics as
/// `backup_write_verify_json` but for TOML (codex config.toml).
pub fn backup_write_verify_toml(
    path: &Path,
    bytes: &[u8],
    mode: u32,
) -> Result<Option<String>, String> {
    let backup =
        backup_and_atomic_write(path, bytes, mode).map_err(|e| format!("write: {e}"))?;
    if let Err(msg) = read_back_verify_toml(path) {
        restore_backup_or_remove(path, backup.as_deref());
        return Err(format!("verify failed, restored: {msg}"));
    }
    Ok(backup)
}

// ── timestamp (UTC, no chrono) ────────────────────────────────────

/// Format the current UTC time as `YYYYMMDDTHHMMSSmmm` (fixed-width,
/// lexicographically sortable). Millis disambiguate backups taken within
/// the same second (four rapid applies in a test would otherwise collide).
fn format_timestamp_utc() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();

    let days = (secs / 86400) as i64;
    let sec_in_day = secs % 86400;
    let hour = sec_in_day / 3600;
    let min = (sec_in_day % 3600) / 60;
    let sec = sec_in_day % 60;
    let (year, month, day) = days_to_ymd(days);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}{:03}",
        year, month, day, hour, min, sec, millis
    )
}

/// Convert days-since-Unix-epoch (1970-01-01 = day 0) to (year, month, day)
/// in the proleptic Gregorian calendar. Matches `chrono::Utc::now()` for all
/// reachable dates; kept inline to avoid adding the `time`/`chrono` crate.
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

// ── assignment resolution ──────────────────────────────────────────

/// The four agent tabs. Used by the live-readback reader so each agent
/// can map its own native config file shape into a uniform `live` JSON.
#[derive(Debug, Copy, Clone)]
pub enum Agent {
    Pi,
    Opencode,
    Claude,
    Codex,
}

impl Agent {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pi" => Some(Self::Pi),
            "opencode" => Some(Self::Opencode),
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_dir() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("aio-render-test-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    // --- format_timestamp_utc ---

    #[test]
    fn timestamp_is_fixed_width_and_lex_sortable() {
        let a = format_timestamp_utc();
        // YYYYMMDDTHHMMSSmmm = 4+2+2+1+2+2+2+3 = 18 chars, fixed width.
        assert_eq!(a.len(), 18, "expected 18-char timestamp, got {a}");
        assert!(a.contains('T'));
        // Must be all ASCII digits + T at index 8.
        let bytes = a.as_bytes();
        assert_eq!(bytes[8], b'T');
        for (i, b) in bytes.iter().enumerate() {
            if i == 8 {
                continue;
            }
            assert!(b.is_ascii_digit(), "non-digit at {i}: {a}");
        }
    }

    // --- days_to_ymd ---

    #[test]
    fn epoch_day_zero_is_1970_01_01() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn known_dates_round_trip() {
        // 2026-01-01 is 20454 days after 1970-01-01.
        assert_eq!(days_to_ymd(20454), (2026, 1, 1));
        // 2026-08-26 = 20691 days.
        assert_eq!(days_to_ymd(20691), (2026, 8, 26));
    }

    #[test]
    fn leap_year_february() {
        // 2024-02-29 is 19782 days after 1970-01-01. Non-leap years skip 2/29.
        assert_eq!(days_to_ymd(19782), (2024, 2, 29));
        // 2025-02-28 = 20147.
        assert_eq!(days_to_ymd(20147), (2025, 2, 28));
        // 2023-03-01 = 19417.
        assert_eq!(days_to_ymd(19417), (2023, 3, 1));
    }

    // --- backup_file + prune ---

    #[test]
    fn backup_returns_none_when_missing() {
        let dir = temp_dir();
        let path = dir.join("missing.json");
        assert_eq!(backup_file(&path).unwrap(), None);
    }

    #[test]
    fn backup_copies_and_prunes_to_newest_3() {
        let dir = temp_dir();
        let path = dir.join("f.json");

        // Write 4 versions in a loop, backing up between each. Because millis
        // disambiguate names, sleeping 1ms between writes is enough.
        for i in 0..4 {
            std::fs::write(&path, format!("v{i}")).unwrap();
            // Force distinct millis: sleep 2ms.
            std::thread::sleep(std::time::Duration::from_millis(2));
            let _ = backup_file(&path).unwrap();
        }
        // Count remaining backups.
        let prefix = "f.json.aio-bak-";
        let count = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(prefix)
            })
            .count();
        assert_eq!(count, 3, "should keep newest 3 backups");
    }

    // --- atomic_write ---

    #[test]
    fn atomic_write_creates_file_with_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir();
        let path = dir.join("nested/deep/secret.json");
        atomic_write(&path, b"{}", 0o600).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = temp_dir();
        let path = dir.join("f.json");
        std::fs::write(&path, "old").unwrap();
        atomic_write(&path, b"new", 0o644).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn atomic_write_leaves_no_tmp_behind() {
        let dir = temp_dir();
        let path = dir.join("f.json");
        atomic_write(&path, b"{}", 0o644).unwrap();
        let has_tmp = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains("aio-tmp"));
        assert!(!has_tmp);
    }

    // --- read_json_object ---

    #[test]
    fn read_missing_is_none() {
        let dir = temp_dir();
        let path = dir.join("nope.json");
        assert!(matches!(read_json_object(&path), Ok(None)));
    }

    #[test]
    fn read_empty_is_none() {
        let dir = temp_dir();
        let path = dir.join("empty.json");
        std::fs::write(&path, "   \n").unwrap();
        assert!(matches!(read_json_object(&path), Ok(None)));
    }

    #[test]
    fn read_object_is_some() {
        let dir = temp_dir();
        let path = dir.join("ok.json");
        std::fs::write(&path, r#"{"a":1}"#).unwrap();
        let v = read_json_object(&path).unwrap().unwrap();
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn read_corrupt_is_err() {
        let dir = temp_dir();
        let path = dir.join("bad.json");
        std::fs::write(&path, "{{not json").unwrap();
        assert!(matches!(read_json_object(&path), Err(ReadError::Corrupt(_))));
    }

    #[test]
    fn read_non_object_is_corrupt() {
        let dir = temp_dir();
        let path = dir.join("arr.json");
        std::fs::write(&path, "[1,2,3]").unwrap();
        assert!(matches!(read_json_object(&path), Err(ReadError::Corrupt(_))));
    }

    // --- ApplyResult ---

    #[test]
    fn apply_result_remove_written_drops_match() {
        let mut r = ApplyResult::new();
        r.push_ok("/a/b.json".into(), None);
        r.push_ok("/a/c.json".into(), None);
        assert!(r.remove_written("/a/b.json"));
        assert_eq!(r.written.len(), 1);
        assert!(!r.remove_written("/a/b.json"));
    }

    #[test]
    fn push_err_flips_ok() {
        let mut r = ApplyResult::new();
        assert!(r.ok);
        r.push_err("/x".into(), "boom".into());
        assert!(!r.ok);
        assert_eq!(r.errors.len(), 1);
    }
}
