// Background model-config pull from sandbox-mgr (Phase 4b, design §3.7).
//
// When this sandbox is managed by sandbox-mgr (MGR_URL set in the
// mgr-generated compose), mgr is the single source of truth for the model
// config (D6). This task pulls `GET {MGR_URL}/api/models/sync` every 60s
// (plus once at startup), deep-compares the payload against the local
// canonical store, and on difference OVERWRITES the local store and
// re-renders every assigned agent's native files through the same apply
// pipeline a user-triggered apply uses (routes::models::apply_all_agents —
// the pull path never owns a second render pipeline).
//
// Failure semantics (mgr down / offline / non-200 / unparseable payload):
// one tracing::warn per failure, then keep using the local cache — never
// write, never panic, never exit the task. A CORRUPT local store is
// overwritten by the mgr copy (whole-document override; mgr is the
// authority, so the pull self-heals — unlike the PUT path, which moves a
// corrupt file aside for a human to look at).
//
// Concurrency: the compare+write+render pass holds `state.models_lock`,
// the same lock every /api/models handler takes (design §3), so a pull can
// never interleave with a user write. Under MGR_URL the local write
// endpoints are 403 anyway (managed_guard); the lock keeps that invariant
// true by construction rather than by convention.
//
// The payload's `version` (mgr kv store version, +1 per mgr write) is a
// plain ETag surrogate; the compare itself is a deep serde_json value
// compare of the whole config, so a missed/rolled-back version can never
// cause a skipped sync.

use std::path::Path;

use aio_models::store::{read_config, write_config, CanonicalConfig, StoreError};
use serde::Deserialize;

use crate::routes::models::apply_all_agents;
use crate::routes::models::render::home_dir;
use crate::state::AppState;

/// Pull period (design §3.7: startup + every 60s).
const SYNC_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
/// Per-request timeout: a hung mgr must not pile up requests in this task.
const SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// GET /api/models/sync payload shape (mgr/src/models.rs): the mgr-side kv
/// version + the UNMASKED canonical config — the sandbox needs the real
/// keys to render the agents' native files.
#[derive(Debug, Deserialize)]
struct SyncPayload {
    #[serde(default)]
    version: u64,
    config: CanonicalConfig,
}

/// Spawn the pull loop. Only called when MGR_URL is set (main.rs); runs
/// forever — individual cycle failures are warned and swallowed below.
pub fn spawn_mgr_sync(state: AppState) {
    tokio::spawn(async move {
        tracing::info!(
            "mgr sync: model-config pull task started ({} / 60s)",
            state.mgr_url.as_deref().unwrap_or_default()
        );
        loop {
            if let Err(e) = sync_once(&state).await {
                tracing::warn!("mgr sync: {e}; keeping local cache");
            }
            tokio::time::sleep(SYNC_INTERVAL).await;
        }
    });
}

/// One pull-and-apply cycle. Ok(true) = the local store was overwritten and
/// re-rendered; Ok(false) = no change; Err = fetch/local-read failure (the
/// local cache is guaranteed untouched on Err).
pub(crate) async fn sync_once(state: &AppState) -> Result<bool, String> {
    let Some(mgr_url) = state.mgr_url.as_deref() else {
        return Err("MGR_URL unset".to_string());
    };
    let payload = fetch(state, mgr_url).await?;

    // Serialize with every other models.json reader/writer (same lock the
    // handlers take; see module doc on concurrency).
    let _guard = state.models_lock.lock().await;

    let local = match read_config(&state.models_file) {
        Ok(c) => c,
        // Corrupt local + valid mgr copy = mgr wins (module doc: self-heal).
        Err(StoreError::Corrupt(e)) => {
            tracing::warn!("mgr sync: local models.json corrupt ({e}); overwriting from mgr");
            CanonicalConfig::default() // differs from any non-empty mgr copy
        }
        Err(StoreError::Io(e)) => return Err(format!("read local models.json: {e}")),
    };
    if !config_differs(&local, &payload.config) {
        return Ok(false);
    }

    overwrite_and_render(&state.models_file, &home_dir(), &payload.config)
        .map_err(|e| format!("write models.json: {e}"))?;
    tracing::info!(
        "mgr sync: applied new model config from mgr (store version {})",
        payload.version
    );
    Ok(true)
}

/// GET {mgr_url}/api/models/sync and decode. Transport / non-200 / parse
/// failures are Err (the caller warns once and keeps the local cache).
async fn fetch(state: &AppState, mgr_url: &str) -> Result<SyncPayload, String> {
    let url = format!("{}/api/models/sync", mgr_url.trim_end_matches('/'));
    let resp = state
        .http
        .get(&url)
        .timeout(SYNC_TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("GET {url}: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("GET {url}: HTTP {status}"));
    }
    resp.json::<SyncPayload>()
        .await
        .map_err(|e| format!("GET {url}: decode response: {e}"))
}

/// Deep compare via serde_json values — faithful to the single-document
/// override semantics ("the whole document changed"): order-insensitive
/// for the BTreeMap providers, and null-vs-absent normalizes the same way
/// on both sides (both values come from the same serde schema).
pub(crate) fn config_differs(local: &CanonicalConfig, remote: &CanonicalConfig) -> bool {
    match (serde_json::to_value(local), serde_json::to_value(remote)) {
        (Ok(a), Ok(b)) => a != b,
        // Serializing this schema cannot fail (no non-string map keys, no
        // unsupported values); treat an impossible failure as "differs" so
        // the write path re-asserts mgr's copy as the local truth.
        _ => true,
    }
}

/// Overwrite the local store with mgr's copy, then re-render every assigned
/// agent. Render runs ONLY after a successful write (a failed write must
/// not leave the native files claiming config the store doesn't have).
/// Caller must hold models_lock. Per-agent render failures are warned and
/// skipped — one agent's broken native file must not block the others.
pub(crate) fn overwrite_and_render(
    models_file: &Path,
    home: &Path,
    remote: &CanonicalConfig,
) -> Result<(), std::io::Error> {
    write_config(models_file, remote)?;
    for (agent, result) in apply_all_agents(remote, home) {
        if result.ok {
            tracing::info!(
                "mgr sync: rendered {agent} native config ({} file(s))",
                result.written.len()
            );
        } else {
            let errs: Vec<&str> = result.errors.iter().map(|e| e.message.as_str()).collect();
            tracing::warn!("mgr sync: render {agent} failed: {}", errs.join("; "));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aio_models::store::{AgentAssignment, ModelEntry, ProviderEntry};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_dir() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("aio-mgr-sync-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn sample_config(api_key: &str) -> CanonicalConfig {
        let mut c = CanonicalConfig::default();
        c.providers.insert(
            "prov-a".to_string(),
            ProviderEntry {
                name: "Prov A".into(),
                base_url: "https://a.example/v1".into(),
                api_key: Some(api_key.to_string()),
                models: vec![ModelEntry {
                    id: "model-a".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        c.agents.pi = Some(AgentAssignment {
            provider: "prov-a".into(),
            model: "model-a".into(),
        });
        c
    }

    #[test]
    fn config_differs_false_for_equal_and_true_for_any_change() {
        let a = sample_config("sk-key");
        let b = sample_config("sk-key");
        assert!(!config_differs(&a, &b), "identical configs must not differ");

        // Key change (the masked-echo trap: a masked key would be a silent
        // difference — the pull compares the REAL values).
        let masked = sample_config("sk-****-key");
        assert!(config_differs(&a, &masked));

        // Assignment change.
        let mut assigned = sample_config("sk-key");
        assigned.agents.pi = None;
        assert!(config_differs(&a, &assigned));

        // Provider added/removed.
        let mut extra = sample_config("sk-key");
        extra.providers.insert(
            "prov-b".to_string(),
            ProviderEntry::default(),
        );
        assert!(config_differs(&a, &extra));

        // Empty local (fresh sandbox) differs from a non-empty mgr config.
        assert!(config_differs(&CanonicalConfig::default(), &a));
    }

    #[test]
    fn config_differs_ignores_map_insertion_order() {
        // BTreeMap iteration order is deterministic, but prove the compare
        // is structural, not textual: build two configs with opposite
        // insertion orders of the same providers.
        let mut a = CanonicalConfig::default();
        let mut b = CanonicalConfig::default();
        for (m, dst) in [
            (vec![("prov-a", 1u8), ("prov-b", 2), ("prov-c", 3)], &mut a),
            (vec![("prov-c", 3), ("prov-a", 1), ("prov-b", 2)], &mut b),
        ] {
            for (id, n) in m {
                dst.providers.insert(
                    id.to_string(),
                    ProviderEntry {
                        name: format!("P{n}"),
                        ..Default::default()
                    },
                );
            }
        }
        assert!(!config_differs(&a, &b));
    }

    #[test]
    fn sync_payload_decodes_mgr_wire_shape() {
        // Lock the wire contract with mgr/src/models.rs sync(): the payload is
        // exactly {version, config} — camelCase provider fields (aio-models
        // serde), version defaults to 0 when absent.
        let j = serde_json::json!({
            "version": 3,
            "config": {
                "providers": {
                    "prov-a": {
                        "name": "Prov A",
                        "baseUrl": "https://a.example/v1",
                        "api": "openai-completions",
                        "apiKey": "sk-real",
                        "models": [{"id": "model-a"}]
                    }
                },
                "agents": {"pi": {"provider": "prov-a", "model": "model-a"}}
            }
        });
        let p: SyncPayload = serde_json::from_value(j).expect("mgr sync shape decodes");
        assert_eq!(p.version, 3);
        assert_eq!(p.config.providers["prov-a"].api_key.as_deref(), Some("sk-real"));
        assert_eq!(p.config.agents.pi.as_ref().unwrap().model, "model-a");

        // version absent (older mgr) defaults instead of failing the pull.
        let j = serde_json::json!({ "config": {} });
        let p: SyncPayload = serde_json::from_value(j).expect("version defaults");
        assert_eq!(p.version, 0);
    }

    #[test]
    fn overwrite_and_render_writes_store_and_renders_assigned_agents() {
        let dir = temp_dir();
        let models_file = dir.join("models.json");
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();

        let remote = sample_config("sk-real-key");
        overwrite_and_render(&models_file, &home, &remote).unwrap();

        // Store: mgr's copy lands verbatim (real key, not a mask).
        let back = read_config(&models_file).unwrap();
        assert!(config_json_equal(&back, &remote));
        assert_eq!(
            back.providers["prov-a"].api_key.as_deref(),
            Some("sk-real-key")
        );
        // Render: pi (the only assigned agent) got its native files.
        let settings = std::fs::read_to_string(home.join(".pi/agent/settings.json")).unwrap();
        assert!(settings.contains("prov-a"));
    }

    #[test]
    fn overwrite_and_render_skips_render_when_store_write_fails() {
        // A write failure (parent path occupied by a regular file) must
        // abort BEFORE any native file is rendered — never half-applied.
        let dir = temp_dir();
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, "regular file").unwrap();
        let models_file = blocker.join("nested/models.json"); // NotADirectory
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();

        let err = overwrite_and_render(&models_file, &home, &sample_config("sk"));
        assert!(err.is_err());
        assert!(
            !home.join(".pi/agent/settings.json").exists(),
            "render must not run after a failed store write"
        );
    }

    /// serde_json deep equality (mirror of the module's compare).
    fn config_json_equal(a: &CanonicalConfig, b: &CanonicalConfig) -> bool {
        serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
    }
}
