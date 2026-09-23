# Research: model config sync chain — single global config → multi-profile conversion touch points

- **Query**: app/src/mgr_sync.rs pull loop mechanics; mgr/src/models.rs kv storage; aio-models store API; ALL touch points for converting single global model config → multi-profile + per-sandbox assignment (DB, API, sync request/response, sandbox pull, mgr-web models pages, UsagePage fan-out).
- **Scope**: internal
- **Date**: 2026-09-09

## 1. app/src/mgr_sync.rs — pull loop mechanics (exact)

- **Timer**: `SYNC_INTERVAL = 60s` (mgr_sync.rs:40), `SYNC_TIMEOUT = 5s` per request (:42). Loop: `sync_once` then sleep 60s, forever (mgr_sync.rs:62-68). Startup pull included (first iteration runs immediately). Spawned only when `MGR_URL` set (app/src/main.rs:111-113).
- **Endpoint called**: `GET {mgr_url}/api/models/sync` (mgr_sync.rs:109); `mgr_url` trims trailing `/`. Transport/non-200/parse failure → `Err` → **one `tracing::warn!("mgr sync: {e}; keeping local cache")` per cycle, keep local cache, never write, never exit** (mgr_sync.rs:63-66). No backoff.
- **Payload decode**: `SyncPayload { version: u64 (serde default 0), config: CanonicalConfig }` (mgr_sync.rs:47-52). Wire shape locked both directions by handler-level tests: mgr `sync_handler_shape_is_version_plus_unmasked_config` (mgr/src/models.rs:1306-1325) and app `sync_payload_decodes_mgr_wire_shape` (mgr_sync.rs:255-284).
- **Compare**: holds `state.models_lock` (same lock every /api/models handler takes — mgr_sync.rs:82; module doc :19-23), reads local via `read_config(&state.models_file)`:
  - Corrupt local (StoreError::Corrupt) → treated as default → mgr copy OVERWRITES (self-heal; unlike the PUT path which moves corrupt aside) — mgr_sync.rs:86-92.
  - `config_differs(local, remote)` = deep serde_json value compare, order-insensitive, impossible-serialize-failure → "differs" (mgr_sync.rs:130-138). `version` is only an ETag surrogate (:26-28).
- **Write path**: `overwrite_and_render(models_file, home, remote)` (mgr_sync.rs:145-163): `write_config` FIRST, then `apply_all_agents` (shared with the apply handler — single render pipeline, no second one). Render only after successful store write (:141-144 comment). Per-agent render failures warned + skipped, never abort siblings.
- `apply_all_agents` lives in app/src/routes/models/mod.rs:312-327: renders every agent WITH an assignment (pi/opencode/claude/codex), skips absent assignments.

## 2. mgr/src/models.rs — storage schema & endpoints

### kv storage

- Row key: **`KV_MODELS = "models_config"`** (models.rs:54).
- Stored value: `StoredModels { version: u64, config: CanonicalConfig }` serialized as JSON (models.rs:61-65). `version` starts 0 on never-written (read_stored default, models.rs:73-76 — wait: default is `version: 0` in the struct default path; first PUT writes version 1, test at models.rs:1180-1199), +1 per successful mutation.
- Read semantics: missing row = default config + version 0 (models.rs:74-76); corrupt row = internal error, NEVER a reset (models.rs:77-81, test :1242-1251).
- Read-modify-write happens entirely under one `state.db` Mutex acquisition with no await between (put_config models.rs:163-189; module doc :85-91) — concurrent PUTs serialize naturally; kv replaces app's models.json + models_lock.
- kv helpers: `db::kv_set/kv_get` (mgr/src/db.rs:232-251). Schema `CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT)` (db.rs:55).

### endpoints (models.rs router, :133-141)

| Route | Handler | Behavior |
|---|---|---|
| GET /api/models/config | `get_config` (:147-154) | read_stored → `mask_config` → return masked CanonicalConfig |
| PUT /api/models/config | `put_config` (:162-189) | merge_api_keys (masked-echo: absent=keep, ""=clear, mask=keep, other=replace) → ensure_preset_ids → validate → `incoming.version = 1` → write_stored(version+1) → `PutResponse {ok: true, warnings: []}` |
| GET /api/models/sync | `sync` (:205-214) | **UNMASKED** `{version, config}` — plaintext keys; security boundary documented at :196-204 (D6/D9: trust boundary = host machine; :8089 unpublished, aio-mgr-net host-local) |
| POST /api/models/import/pi | `import_pi` (:223-262) | `MGR_PI_MODELS_FILE` env or `$HOME/.pi/agent/models.json`; NotFound→404 w/ containerized-form hint; inserts providers, version+1 |
| POST /api/models/discover | `discover` (:374-461) | untagged body `{providerId}` or `{baseUrl, api?, apiKey?}`; multi-candidate URL fallback (candidate_urls :507-532), protocol-adaptive headers (:537-571), multi-shape parse (:579-634); 20s per-candidate + 20s total budget (:327-329); first-candidate 401/403 short-circuits |
| POST /api/models/test | `test` (:755-849) | `{providerId, modelId, protocol?}` camelCase; completion probe `max_tokens 16`, prompt "Reply with OK only.", 20s timeout; always HTTP 200 with ok:false semantics |
| GET /api/models/catalog | `get_catalog` (:1020-1059) | models.dev `https://models.dev/api.json` proxy, 1h cache, in-flight dedup via tokio Mutex (:1003-1016) |

- NOT in mgr (sandbox-local semantics, 契约 7 list at models.rs:22-24 + sandbox-mgr-ops.md:208-210): `/api/models/agents`, `apply/:agent`, live-provider PUT/DELETE, `agents/:agent/sync`, per-sandbox usage.
- Error style: local ApiError mirrors routes.rs `{"error": msg}` (models.rs:101-129) — app uses `(StatusCode, String)` plain text; mgr-web's apiError decodes both.

## 3. aio-models store API surface (aio-models/src/store.rs)

Types:
- `CanonicalConfig { version: u32 (always 1 on write), providers: BTreeMap<String, ProviderEntry>, agents: AgentsConfig }` (store.rs:22-30).
- `AgentsConfig { pi?, opencode?: AgentAssignment, claude?: ClaudePresets, codex?: CodexPresets }` (store.rs:46-56).
- `AgentAssignment { provider, model }` camelCase (store.rs:58-63).
- `ClaudePresets { presets: Vec<ClaudePreset>, current: Option<String> }` + shadow deserializer for legacy shape (store.rs:74-80, :82+); `ClaudePreset { id (backend backfill), name, provider, model, haiku/sonnet/opusModel?, authField: "AUTH_TOKEN" }` (store.rs:84-101).
- `CodexPresets`/`CodexPreset { id, name, provider, model, reasoningEffort?, wireApi }` (store.rs:180-207).
- `ProviderEntry` (store.rs:334), `ModelEntry` (:357), `CostEntry` (:381), `PutResponse` (:395), `ImportResponse` (:401), `StoreError` (:410).

Functions (file:line):
- `read_config(path)` (:432) — missing file = default; `write_config(path, config)` (:448) — atomic temp+rename, 0600, pretty JSON.
- `mask_key` (:469) — first3+"****"+last4 when len>=8; `mask_config` (:481) — masks every provider apiKey.
- `merge_api_keys(stored, incoming)` (:499-520) — the masked-echo merge.
- `validate(config)` (:525-560) — provider id `[a-z0-9-]+`; assignment provider/model must resolve (:569-591); preset validation incl. unique ids + non-dangling current (:597-644).
- `gen_preset_id` (:263), `ensure_preset_ids` (:317).
- `import_from_pi` (:723), `import_pi_provider` (:734), `import_from_opencode` (:793), `import_opencode_provider` (:804), `ImportResult` (:689).

**Single-owner rule**: aio-models is shared by app AND mgr (aio-models/src/lib.rs:1-8) — any schema change (e.g. profiles) automatically propagates to both; the migration surface is: this crate (types+validate), the mgr kv wrapper, the app's local models.json on every sandbox volume, and the two frontend type mirrors.

## 4. Env wiring (who sets MGR_URL / reads it)

- mgr-side injection: composegen.rs:91 `MGR_URL: http://mgr-api:8089` in the generated app environment.
- app-side read: `config::mgr_url()` (app/src/config.rs:264-266) — read once at startup (env constant for process lifetime), `Option<String>` filtering empty.
- AppState carries `mgr_url` (app/src/state.rs:37); `managed_guard` 403s every /api/models write when set (app/src/routes/models/mod.rs:74-79); `GET /api/models/managed` exposes `{"managed": bool}` (:66-69).

## 5. ALL touch points for multi-profile + per-sandbox assignment (enumeration)

### A. Data layer

1. **mgr kv row** — `KV_MODELS "models_config"` single row (mgr/src/models.rs:54, StoredModels :61-65). Multi-profile needs either:
   - one row per profile (`models_config:<profile-id>`) — read_stored/write_stored + KV_MODELS const (:73-93); or
   - a new schema `{version, profiles: {id: CanonicalConfig}, assignments: {sandbox: profile_id}}`.
   - The kv table is generic (db.rs:55, kv_set/kv_get :232-251) — new keys need no migration.
2. **Sandbox→profile assignment storage** — natural home: a new kv row (e.g. `models_assignments`) or a column on the `sandboxes` table (db.rs:28-39 schema is migration-free by design; db.rs:4-6 comment says kv was created upfront to avoid ALTER TABLE churn — prefer kv). `SandboxRow` (db.rs:59-70) + `SANDBOX_COLS` (:86-87) + `sandbox_json` payload (routes.rs:441-459) would all change if a column is chosen.
3. **aio-models schema** — CanonicalConfig/AgentsConfig (store.rs:22-56) + validate (:525-560). If a "profile" is a whole CanonicalConfig, the schema may stay untouched and only the wrapper changes — but per-sandbox `agents` selection then needs per-profile agent blocks.
4. **app local store** — `/root/.aio/models.json` per sandbox (app/src/main.rs:67, state.rs:36). Under D8, each sandbox holds ITS profile's config locally; pull must fetch only its slice.

### B. mgr API (mgr/src/models.rs)

5. GET/PUT `/api/models/config` (:147-189) — becomes profile-scoped (`?profile=` or path).
6. GET `/api/models/sync` (:205-214) — **must know WHICH sandbox is pulling**. Today no auth/no identification: any caller gets the single global config. Per-sandbox assignment requires the request to identify itself — options: composegen injects a per-sandbox URL (`MGR_URL` + sandbox name, e.g. `http://mgr-api:8089` stays but a new `MGR_SANDBOX_NAME` env is injected alongside — composegen.rs:85-91 is where the app env block is generated), and sync takes `?name=` or a header. mgr-side validation: name must exist in sandboxes table (require_row pattern routes.rs:595-599).
7. `import_pi`, `discover`, `test`, `catalog` (:223-1059) — profile-scoping for import; discover/test/catalog are profile-independent (resolve by provider id → becomes per-profile store lookup in resolve_provider :292-322 and test :763-777).
8. Profile CRUD endpoints (new): list/create/rename/delete profile + assignment set/unset. Router :133-141 + routes.rs merge point :46-47.

### C. Sync request/response contract

9. `SyncPayload {version, config}` (app mgr_sync.rs:47-52) ↔ mgr sync handler (models.rs:205-214) — locked by test pairs on both sides (mgr models.rs:1306-1325; app mgr_sync.rs:255-284). Response becomes the sandbox's assigned-profile config; `version` semantics become per-profile (version bump on any profile write must only trigger pulls of sandboxes assigned to that profile — or version becomes per-profile and the deep compare already tolerates any version drift since compare is content-based, mgr_sync.rs:26-28, :130-138).
10. Wire-shape locks in tests: mgr `put_semantics_version_increments_each_write`, `get_config_handler_returns_masked_keys`, `put_config_handler_merge_validate_and_version_bump` (models.rs:1180-1376) — all assume ONE store.

### D. Sandbox-side pull

11. `mgr_sync.rs` whole file: fetch URL (:109), lock+compare+overwrite (:74-104). Multi-profile means the response is already the sandbox's assigned config → the file itself needs NO change if mgr returns the right slice; only the request must identify the sandbox.
12. composegen env injection (mgr/src/composegen.rs:87-91 comment + :91) — inject sandbox identity (`MGR_URL` already implies the sandbox runs under mgr; adding e.g. `MGR_SANDBOX_NAME` or baking the name into `MGR_URL` path). Adopted stacks (routes.rs:344-346 adopt only network-connects) have NO MGR_URL — their model config is unmanaged (env_json empty by design, routes.rs:349-350); D8 must decide whether adopted sandboxes participate.
13. `managed_guard` + `get_managed` (app/src/routes/models/mod.rs:66-79) — unchanged semantics.

### E. mgr-web models pages

14. **mgr-web/src/pages/models/ModelsPage.tsx** (859 lines) — owns ALL state + handlers; tab union `TabKey = "providers" | "pi" | "opencode" | "claude" | "codex"` (:61). A per-sandbox assignment UI slots naturally as: (a) a new "sandboxes/assignment" tab in TAB_KEYS (:63), (b) a profile selector in the page head (:742-745), or (c) both. State `config: CanonicalConfig | null` (:82) becomes per-selected-profile.
15. **api.ts models section** (mgr-web/src/api.ts:107-146): `getModelsConfig/putModelsConfig/importPiModels/discoverModels/testModel/getModelsCatalog/getUsage` — all need profile params; new profile + assignment functions follow the `get`/`send` pattern (:34-48).
16. **types.ts models mirror** (mgr-web/src/pages/models/types.ts:19-114): CanonicalConfig/ProviderEntry/agents/presets/PutResponse/ImportResponse — mirrors aio-models. New profile/assignment interfaces join here (single boundary owner rule, file header :1-16).
17. **Sub-components**: ProviderGrid.tsx (150), ProviderEditor.tsx (494), PresetList.tsx (455), AgentTabs.tsx (159), ModelPicker.tsx (57), ModelRow.tsx (250), charts.tsx (146) — these consume `CanonicalConfig` and are profile-agnostic (they'd render whatever profile is selected); MgrNotice.tsx (41) links to sandbox entry_urls (:13-15) — under the unified workspace it links to the mgr-web workspace instead.
18. **EditPage.tsx** — per-sandbox settings page (env picker + cpus/mem fields, mgr-web/src/pages/EditPage.tsx:126-186). A per-sandbox model-profile ASSIGNMENT selector slots in the `wizard` div next to the resources field-row (:146-172); PUT body currently `{env, cpus, mem_mb}` (:103-107) — would gain a `model_profile` field (mgr-web/src/types.ts PutBody :93-97 + mgr routes.rs PutBody :485-490 + put_sandbox merge :504-521).
19. **SandboxListPage.tsx** — card could display assigned profile (sbx-meta block :232-249); entry button (:219-229) currently opens `entry_url` in a NEW TAB (D10) — under D2 this becomes "open in workspace tab".

### F. Usage fan-out

20. **mgr/src/usage.rs** — fan-out URL `http://sbx-{name}-piweb:8088/api/models/usage?window=` (:158), 5s timeout (:55), 30s TTL cache keyed `<name>:<window>` (:57, state.rs CachedUsage :53-62). Usage stays per-sandbox already (each sandbox aggregates its own logs locally in app/src/routes/models/usage.rs) — multi-profile does NOT change the fan-out mechanics; only the mgr-web presentation.
21. **mgr-web/src/pages/UsagePage.tsx** — sandbox chips (:143-169), combined/all view, sandbox column in the table (:230, :245). No model-profile dimension exists today; a profile grouping column would be additive (rows already carry `agent/provider/model`, UsageRow in models/types.ts).

### G. web/ workbench (deprecated by D1)

22. **web/src/panes/models/** — the entire workbench models pane (ModelsPane.tsx 1211 lines + subcomponents) dies with D1; its managed-mode banner (`mcManagedBanner`, web/src/i18n.ts:208-209) + read-only flip (ModelsPane.tsx:94, :167-197) become irrelevant — mgr-web becomes the only editor.

### H. Specs to update

23. `.trellis/spec/backend/sandbox-mgr-ops.md` 契约 7 (lines 166-210) — kv shape, sync chain, write-degradation matrix all describe the single-config world.
24. `.trellis/spec/backend/api-contracts.md` mgr section (lines 201-265) — no models endpoints documented there yet (they're in 契约 7); new profile endpoints should be added.
25. `.trellis/spec/backend/model-config-guide.md` — exists in spec/backend (not deep-read for this note; likely describes the single-config schema).

## 6. Caveats / Not Found

- `.trellis/spec/backend/model-config-guide.md` was NOT read (out of the requested topic list) — check before writing design.md; it likely holds the canonical single-config contract text that D8 amends.
- The version-bump-on-every-write means every sandbox pull re-compares (content-equal → no write). Multi-profile with a single global version would cause no-op compares on unrelated sandboxes; per-profile versions avoid even the compare cost. Content compare makes correctness unaffected either way (mgr_sync.rs:26-28).
- No existing mechanism identifies WHICH sandbox is calling mgr (`MGR_URL` is identical for all sandboxes: `http://mgr-api:8089`, composegen.rs:91). This is the one genuinely new contract in the sync chain.
- mgr-web has no per-sandbox polling of model config; the ModelsPage fetches once per mount + after saves.
