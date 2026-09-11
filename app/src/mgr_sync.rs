// Background model-config pull from sandbox-mgr (Phase 4b, design §3.7;
// unified Phase 4/D8: profile-scoped pull; S2/09-10-mgr-models-agent-assign
// design §3: agent-subset render).
//
// When this sandbox is managed by sandbox-mgr (MGR_URL set in the
// mgr-generated compose), mgr is the single source of truth for the model
// config (D6). This task pulls `GET {MGR_URL}/api/models/sync?name=<this
// sandbox>` every 60s (plus once at startup) — mgr resolves the sandbox's
// ASSIGNED profile (D8) and answers with that profile's config + the
// sandbox's ASSIGNED AGENT SUBSET (S2: `agents: null` = all four, legacy;
// `agents: [names]` = render only those; zero/none assigned = mgr 404s,
// which lands on the keep-local path) — deep-compares the payload against
// the local canonical store AND the last-applied agent subset, and on
// difference OVERWRITES the local store and re-renders the subset's
// assigned agents' native files through the same apply pipeline a
// user-triggered apply uses (routes::models::apply_selected_agents — the
// pull path never owns a second render pipeline; the four renderers are
// untouched, R4).
//
// Agents OUTSIDE the subset are not rendered and not cleaned up — their
// last-written native files stay (PRD R3: 未指派 agent 的本地配置不动).
//
// Failure semantics, two tiers (design §4.3):
//   - 404 (sandbox unassigned / zero-agent assignment / unknown / no name
//     sent) = NOT an error: one tracing::debug per cycle, keep the local
//     cache. This is the unbind contract — after unassignment the sandbox
//     keeps whatever it last pulled, silently.
//   - everything else (mgr down / offline / non-200 / unparseable payload):
//     one tracing::warn per failure, then keep using the local cache —
//     never write, never panic, never exit the task.
// A CORRUPT local store is overwritten by the mgr copy (whole-document
// override; mgr is the authority, so the pull self-heals — unlike the PUT
// path, which moves a corrupt file aside for a human to look at).
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

use crate::routes::models::{apply_selected_agents, parse_agent_subset};
use crate::routes::models::render::{home_dir, Agent};
use crate::state::AppState;

/// Pull period (design §3.7: startup + every 60s).
const SYNC_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
/// Per-request timeout: a hung mgr must not pile up requests in this task.
const SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// GET /api/models/sync payload shape (mgr/src/models.rs): the mgr-side kv
/// version + the UNMASKED canonical config of the ASSIGNED profile — the
/// sandbox needs the real keys to render the agents' native files.
/// `agents` (S2, 09-10-mgr-models-agent-assign, design §3.1) is the sandbox's
/// agent-subset assignment: None/absent (older mgr) = ALL four agents (legacy
/// semantics — old mgr + new app is the AC4 no-regression path); Some(subset)
/// = render only that subset. Some([]) never reaches here — mgr maps a
/// zero-agent assignment to 404 (unassigned), the keep-local path.
#[derive(Debug, Deserialize)]
struct SyncPayload {
    #[serde(default)]
    version: u64,
    config: CanonicalConfig,
    #[serde(default)]
    agents: Option<Vec<String>>,
}

/// One pull cycle's fetch outcome: a payload to apply, or a "mgr says this
/// sandbox has no assigned profile" marker (404 — deliberately distinct
/// from Err so the loop can log it at debug, not warn).
enum Fetched {
    Payload(SyncPayload),
    Unassigned,
}

/// Spawn the pull loop. Only called when MGR_URL is set (main.rs); runs
/// forever — individual cycle failures are warned and swallowed below.
///
/// `last_agents` memory (S2): the loop owns the last-APPLIED agent subset in
/// a loop-local variable — the subset is sync-payload state, not local-store
/// state (the canonical store stays a whole-document override, design §3.3),
/// so cycle-to-cycle comparison is the only place it can live. After a
/// process restart it resets to None: a subset-assigned sandbox re-renders
/// once on the first pull (renderers are key-level merge — idempotent).
pub fn spawn_mgr_sync(state: AppState) {
    tokio::spawn(async move {
        tracing::info!(
            "mgr sync: model-config pull task started ({} / 60s)",
            state.mgr_url.as_deref().unwrap_or_default()
        );
        let mut last_agents: Option<Vec<Agent>> = None;
        loop {
            match fetch(&state).await {
                Ok(Fetched::Unassigned) => {
                    tracing::debug!(
                        "mgr sync: no model profile assigned to this sandbox; keeping local cache"
                    );
                }
                Ok(Fetched::Payload(payload)) => {
                    if let Err(e) = apply(&state, payload, &mut last_agents).await {
                        tracing::warn!("mgr sync: {e}; keeping local cache");
                    }
                }
                Err(e) => {
                    tracing::warn!("mgr sync: {e}; keeping local cache");
                }
            }
            tokio::time::sleep(SYNC_INTERVAL).await;
        }
    });
}

/// One pull-and-apply cycle body. Ok(true) = the local store was
/// overwritten and re-rendered; Ok(false) = no change; Err = local-read or
/// write failure (the local cache is guaranteed untouched on Err).
async fn apply(
    state: &AppState,
    payload: SyncPayload,
    last_agents: &mut Option<Vec<Agent>>,
) -> Result<bool, String> {
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

    // Resolve the subset ONCE per cycle: payload.agents None/absent = all
    // agents (legacy mgr, AC4); Some(names) = whitelist render. Unknown
    // names drop with a warn (parse_agent_subset) — one bad entry from an
    // unknown mgr version never fails the pull.
    let agents: Option<Vec<Agent>> = payload
        .agents
        .as_ref()
        .map(|names| parse_agent_subset(names));

    // Diff covers config AND subset (design §3.2): an unchanged config with
    // a changed subset still re-renders — the subset decides WHICH native
    // files render, so it is part of "what the world should look like".
    // Compare the PARSED subsets (not raw wire names): ["pi","bogus"] and
    // ["pi"] produce the same effective render set, so no re-render is the
    // correct outcome.
    if !config_differs(&local, &payload.config) && *last_agents == agents {
        return Ok(false);
    }

    overwrite_and_render(
        &state.models_file,
        &home_dir(),
        &payload.config,
        agents.as_deref(),
    )
    .map_err(|e| format!("write models.json: {e}"))?;
    *last_agents = agents;
    tracing::info!(
        "mgr sync: applied new model config from mgr (store version {}, agents: {})",
        payload.version,
        match &last_agents {
            None => "all".to_string(),
            Some(list) => list
                .iter()
                .map(|a| agent_name(a).to_string())
                .collect::<Vec<_>>()
                .join(","),
        }
    );
    Ok(true)
}

/// Agent display name for the sync log line (common::Agent has no Display
/// impl — a match here is cheaper than adding one for a single log site).
fn agent_name(a: &Agent) -> &'static str {
    match a {
        Agent::Pi => "pi",
        Agent::Opencode => "opencode",
        Agent::Claude => "claude",
        Agent::Codex => "codex",
    }
}

/// The sync pull URL: `{mgr}/api/models/sync[?name=<sandbox>]`. `?name=` is
/// the pull's identity (design §4.3) — mgr resolves the ASSIGNED profile
/// from it. No name (compose predating MGR_SANDBOX_NAME) keeps the legacy
/// shape; mgr 404s it onto the same silent keep-local path.
fn sync_url(mgr_url: &str, name: Option<&str>) -> String {
    let base = format!("{}/api/models/sync", mgr_url.trim_end_matches('/'));
    match name {
        Some(n) => format!("{base}?name={}", urlencode(n)),
        None => base,
    }
}

/// GET the sync pull and decode. Transport / non-200 (except 404) / parse
/// failures are Err (the caller warns once and keeps the local cache); 404
/// maps to Fetched::Unassigned (debug log, keep local).
async fn fetch(state: &AppState) -> Result<Fetched, String> {
    let Some(mgr_url) = state.mgr_url.as_deref() else {
        return Err("MGR_URL unset".to_string());
    };
    let url = sync_url(mgr_url, state.mgr_sandbox_name.as_deref());
    let resp = state
        .http
        .get(&url)
        .timeout(SYNC_TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("GET {url}: {e}"))?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(Fetched::Unassigned);
    }
    if !status.is_success() {
        return Err(format!("GET {url}: HTTP {status}"));
    }
    resp.json::<SyncPayload>()
        .await
        .map(|payload| Fetched::Payload(payload))
        .map_err(|e| format!("GET {url}: decode response: {e}"))
}

/// Percent-encode a query value (mgr sandbox names are [a-z0-9-] slugs, so
/// this is a formality — kept because the value comes from env, not from a
/// validated route param).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
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

/// Overwrite the local store with mgr's copy, then re-render the assigned
/// agents. `agents: None` = every assigned agent (legacy full pass);
/// `Some(subset)` = only agents in the subset that also have an assignment
/// (S2 — agents outside the sandbox's assignment subset keep their native
/// files untouched, PRD R3). Render runs ONLY after a successful write (a
/// failed write must not leave the native files claiming config the store
/// doesn't have). Caller must hold models_lock. Per-agent render failures
/// are warned and skipped — one agent's broken native file must not block
/// the others.
pub(crate) fn overwrite_and_render(
    models_file: &Path,
    home: &Path,
    remote: &CanonicalConfig,
    agents: Option<&[Agent]>,
) -> Result<(), std::io::Error> {
    write_config(models_file, remote)?;
    for (agent, result) in apply_selected_agents(remote, home, agents) {
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
        // {version, config, agents?} — camelCase provider fields (aio-models
        // serde), version defaults to 0 when absent. The sandbox identity
        // travels in the REQUEST (?name=, see sync_url below), not here.
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
            },
            "agents": ["pi", "opencode"]
        });
        let p: SyncPayload = serde_json::from_value(j).expect("mgr sync shape decodes");
        assert_eq!(p.version, 3);
        assert_eq!(p.config.providers["prov-a"].api_key.as_deref(), Some("sk-real"));
        assert_eq!(p.config.agents.pi.as_ref().unwrap().model, "model-a");
        assert_eq!(
            p.agents.as_deref(),
            Some(&["pi".to_string(), "opencode".to_string()][..]),
        );

        // version AND agents absent (older mgr, S2 design §5: old mgr + new
        // app must not regress) — both default instead of failing the pull;
        // agents None = full render pass.
        let j = serde_json::json!({ "config": {} });
        let p: SyncPayload = serde_json::from_value(j).expect("version/agents default");
        assert_eq!(p.version, 0);
        assert!(p.agents.is_none(), "absent agents = None (all agents)");

        // agents: null on the wire (mgr's full-assignment serialization) is
        // also None, same as absent.
        let j = serde_json::json!({ "version": 1, "config": {}, "agents": null });
        let p: SyncPayload = serde_json::from_value(j).expect("null agents decodes");
        assert!(p.agents.is_none());

        // agents: [] (zero agents — mgr 404s this in practice, but the
        // decoder must not choke if a future mgr sends it) = empty subset,
        // renders nothing.
        let j = serde_json::json!({ "version": 1, "config": {}, "agents": [] });
        let p: SyncPayload = serde_json::from_value(j).expect("empty agents decodes");
        assert_eq!(p.agents.as_deref(), Some(&[][..]));
    }

    #[test]
    fn sync_url_carries_sandbox_name_query() {
        // Request-side wire shape (unified Phase 4, design §4.3): the pull
        // identifies the sandbox via ?name= so mgr resolves the ASSIGNED
        // profile; a trailing slash on MGR_URL never doubles up.
        assert_eq!(
            sync_url("http://mgr-api:8089", Some("alpha")),
            "http://mgr-api:8089/api/models/sync?name=alpha",
        );
        assert_eq!(
            sync_url("http://mgr-api:8089/", Some("alpha")),
            "http://mgr-api:8089/api/models/sync?name=alpha",
        );
        // No name (compose predating MGR_SANDBOX_NAME): legacy shape — the
        // new mgr 404s it onto the silent keep-local path.
        assert_eq!(
            sync_url("http://mgr-api:8089", None),
            "http://mgr-api:8089/api/models/sync",
        );
        // Non-slug bytes percent-encode (defensive; names are slugs).
        assert!(sync_url("http://m", Some("a b")).ends_with("?name=a%20b"));
    }

    #[test]
    fn overwrite_and_render_writes_store_and_renders_assigned_agents() {
        let dir = temp_dir();
        let models_file = dir.join("models.json");
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();

        let remote = sample_config("sk-real-key");
        overwrite_and_render(&models_file, &home, &remote, None).unwrap();

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
    fn overwrite_and_render_subset_excludes_unassigned_agents() {
        // S2 / PRD AC2-AC3: pi AND opencode assigned in the canonical config,
        // but the sandbox's agent subset is [pi] — opencode's native file
        // must stay untouched while pi's renders. The store is still the
        // whole-document override (opencode's ASSIGNMENT lands in
        // models.json; only the RENDER is filtered).
        let dir = temp_dir();
        let models_file = dir.join("models.json");
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();

        let mut remote = sample_config("sk-real-key");
        remote.agents.opencode = Some(AgentAssignment {
            provider: "prov-a".into(),
            model: "model-a".into(),
        });
        let subset = [Agent::Pi];
        overwrite_and_render(&models_file, &home, &remote, Some(&subset)).unwrap();

        // Store: BOTH assignments land (whole-document override).
        let back = read_config(&models_file).unwrap();
        assert!(back.agents.opencode.is_some(), "store keeps the whole config");
        // Render: pi in subset -> rendered; opencode out of subset -> untouched.
        assert!(home.join(".pi/agent/settings.json").exists());
        assert!(
            !home.join(".config/opencode/opencode.jsonc").exists(),
            "agent outside the subset must not be rendered (R3)"
        );
    }

    #[test]
    fn overwrite_and_render_empty_subset_renders_nothing() {
        // Some([]) = zero agents: the store still overwrites (mgr is the
        // authority for the canonical document), but no native file renders.
        // (Mgr maps a zero-agent assignment to 404 in practice — this test
        // pins the app-side behavior were the payload to arrive anyway.)
        let dir = temp_dir();
        let models_file = dir.join("models.json");
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();

        let remote = sample_config("sk");
        overwrite_and_render(&models_file, &home, &remote, Some(&[])).unwrap();

        assert!(read_config(&models_file).is_ok(), "store still written");
        assert!(
            !home.join(".pi/agent/settings.json").exists(),
            "zero-agent subset renders nothing (AC3)"
        );
        assert!(!home.join(".pi/agent/models.json").exists());
    }

    #[test]
    fn overwrite_and_render_subset_unassigned_agent_is_noop_for_it() {
        // Subset names an agent with NO assignment in canonical — the
        // intersection (subset ∩ assigned) drops it; no error, no file.
        let dir = temp_dir();
        let models_file = dir.join("models.json");
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();

        let remote = sample_config("sk");
        // pi assigned; subset asks for claude (unassigned) and pi.
        let subset = [Agent::Claude, Agent::Pi];
        overwrite_and_render(&models_file, &home, &remote, Some(&subset)).unwrap();

        assert!(home.join(".pi/agent/settings.json").exists());
        assert!(!home.join(".claude/settings.json").exists());
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

        let err = overwrite_and_render(&models_file, &home, &sample_config("sk"), None);
        assert!(err.is_err());
        assert!(
            !home.join(".pi/agent/settings.json").exists(),
            "render must not run after a failed store write"
        );
    }

    #[test]
    fn parse_agent_subset_drops_unknown_names() {
        // Unknown agent names are ignored (warn at runtime) — one bad entry
        // from an unknown mgr version never fails the pull.
        let parsed = parse_agent_subset(&[
            "pi".to_string(),
            "bogus".to_string(),
            "codex".to_string(),
        ]);
        assert_eq!(parsed.len(), 2);
        assert!(parsed.contains(&Agent::Pi));
        assert!(parsed.contains(&Agent::Codex));
        assert!(!parsed.contains(&Agent::Claude));

        // All-unknown degrades to the empty subset (renders nothing) —
        // consistent with Some([]) semantics.
        assert!(parse_agent_subset(&["nope".to_string()]).is_empty());
    }

    /// serde_json deep equality (mirror of the module's compare).
    fn config_json_equal(a: &CanonicalConfig, b: &CanonicalConfig) -> bool {
        serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
    }
}
