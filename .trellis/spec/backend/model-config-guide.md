# Unified Model Config Guide

> The `/api/models/*` route group (task 08-26-unified-model-config) — unified
> provider/model config that renders to the native config files of claude code,
> codex, opencode, and pi. Design lineage: pi-web (agegr/pi-web) models panel +
> cc-switch (farion1231/cc-switch) render shapes; research snapshots under
> `.trellis/tasks/08-26-unified-model-config/research/`. claude/codex became
> cc-switch-style multi-preset in task 08-27-agent-multi-preset (see "Agents
> schema" below).

## Route map (`app/src/routes/models/mod.rs`)

| Route | Purpose |
|---|---|
| `GET/PUT /api/models/config` | canonical store read (keys masked) / write (masked-echo merge + preset-id backfill + validate) |
| `POST /api/models/import/pi` | 1:1 import pi's `~/.pi/agent/models.json` providers |
| `POST /api/models/discover` | fetch `/v1/models` from an endpoint (protocol-adaptive URL/headers) |
| `POST /api/models/test` | minimal-completion availability probe |
| `GET /api/models/agents` | per-agent installed (command_exists) + live-config readback |
| `POST /api/models/apply/:agent` | render the agent's current assignment/preset to its files |
| `GET /api/models/usage?window=today\|7d\|all` | per-(agent,model) token aggregation |
| `GET /api/models/catalog` | models.dev metadata catalog, normalized + 1h-cached (08-27-provider-form-piweb) |
| `PUT/DELETE /api/models/agents/:agent/provider/:id` | edit (field-level patch) / delete a provider node in the agent's NATIVE config (08-27-agent-tabs-live-config) |
| `POST /api/models/agents/:agent/sync` | absorb native-config providers into the canonical library (idempotent; body `{id?}` syncs one) |

## Canonical store (`~/.aio/models.json`)

- Single source of truth: `providers{<kebab-id>: {name, baseUrl, api, apiKey, headers, compat, models[]}}` + `agents{pi|opencode: assignment, claude|codex: presets}`.
- `api` ∈ `openai-completions | openai-responses | anthropic-messages`. **No separate `anthropic` override block** (R1): the provider's `baseUrl` IS the endpoint for whatever protocol is selected — an `anthropic-messages` provider's baseUrl is the anthropic URL. Old `models.json` files with a stray `anthropic` key still deserialize (unknown fields ignored) and drop it on next save.
- **Key masking contract**: GET masks `apiKey` as `<first3>****<last4>`; PUT treats `""`=clear, `None`=keep, `<mask>`=keep stored, other=replace. `mask_key` uses char-based slicing (byte slicing panics on non-ASCII keys).
- **Provider id is the universal display key (08-28-provider-id-from-name)**: pi's TUI (model-selector `[id]` badge, footer `(id)`) and pi-web (model-group header) render the provider **id** — the node `name` never reaches those UIs (name only surfaces in pi error strings). So the id must carry the friendly name: while a provider's key is still the auto `provider-N` placeholder (`genProviderId`), filling the name re-keys it to `slug(name)` (ascii-alnum+dash, `-2` suffix on collision, pure-CJK keeps placeholder; `deriveProviderIdFromName` + `rebindAgentProviders` in `types.ts` — custom ids are never rewritten). Renaming a provider that agents already reference must rebind `agents.{pi,opencode}.provider` + `agents.{claude,codex}.presets[].provider` in the same write, delete the old live node on the incremental agents (DELETE `.../agents/:agent/provider/:old`), then re-apply — otherwise the native file keeps a ghost provider node.
- Writes: atomic (tmp+rename), mode 0600, parent 0755. Corrupt file is moved aside (`models.json.corrupt-<ts>`) and surfaced, **never silently overwritten**. `agents.X` blocks are `Option` — unassigned = key absent, never `{}`.

## Agents schema — incremental vs switch-style (08-27)

pi/opencode are **incremental** agents: `agents.{pi,opencode} = {provider, model}`
(one assignment; multiple providers coexist in their native files, the
assignment just sets the default). claude/codex are **switch-style**:
`agents.{claude,codex} = {presets: [...], current: <preset-id>}` — exactly one
preset takes effect (cc-switch model).

- `ClaudePreset {id, name, provider, model, haikuModel?, sonnetModel?, opusModel?, authField}`
  / `CodexPreset {id, name, provider, model, reasoningEffort?, wireApi}`.
  A preset derives from the provider library (providerId + model + overrides) —
  it never copies the provider's key/headers blob; credentials stay in the
  provider library (SSOT). No backfill snapshots (renderers already do
  key-level merge).
- **Preset ids are backend-owned**: `preset-<5 hex>` (splitmix64). PUT
  backfills empty ids (`ensure_preset_ids`, after merge, before validate) and
  resolves the frontend's `current: ""` placeholder ("the preset I just
  created") to the backfilled id. The frontend creates presets with `id: ""`.
- **Backward-compatible migration** lives in deserialization (`ClaudePresetsShadow`):
  an old single-assignment block (`{provider, model, ...}` at the block top
  level) becomes `presets=[{id:"default", name:"默认配置", ...}], current:"default"`.
  No version field; old keys drop out on the next save (same strategy as R1).
- **validate** walks EVERY preset (not just current): unknown provider /
  missing model errors carry the preset name; duplicate preset ids and a
  dangling `current` (set but pointing at no preset) are rejected on PUT.
- Deleting the current preset shifts `current` to the first remaining preset —
  **frontend logic** (ModelsPane `deletePreset`); the backend only guards via
  the dangling-current validation.

## Renderers (`app/src/routes/models/render/`) — key-level merge, never clobber

Each renderer reads the target file (missing ⇒ empty), merges ONLY its keys, backs up to `<file>.aio-bak-<UTCts>` (rolling newest 3), writes atomically (0600), read-backs and verifies parse, and on failure restores from backup. For claude/codex the renderer applies **the current preset** (`ClaudePresets::current_preset()`); when `current` is unset or dangling it pushes `no current <agent> preset` and writes nothing (never a half-applied file).

| agent | target files | merge behavior |
|---|---|---|
| pi | `~/.pi/agent/models.json` + `settings.json` | set `providers.<id>` node + `defaultProvider`/`defaultModel`; other providers & settings keys preserved. **`name` IS rendered (08-28-pi-provider-display-name)**: pi's `ProviderConfigSchema` has optional `name` (minLength 1) and its display-name chain prefers it (`provider-composer.ts`: `config?.name ?? providerId`) — omit it and pi shows the raw id ("provider-1") instead of the user-named provider. `edit_pi_provider` maps `patch.name`: non-empty writes, "" removes the key ("" would fail schema validation). **cost: NO unit conversion (08-28-pi-models-cost-unit)** — pi's native models.json cost is **USD-per-1M-tokens, same as canonical** (pi's usage math: `rate × tokens / 1e6`, pi `packages/ai/src/models.ts`; its generated data matches models.dev verbatim). `render_pi_cost` passes values through as-is, and when any cost field is known it writes **all four** (`input/output/cacheRead/cacheWrite`) with missing ones = `0` — pi's `ModelCostSchema` requires all four whenever `cost` exists (only `tiers` optional), and models.dev commonly omits `cache_write` (e.g. deepseek); pi's own generator uses `cache_write \|\| 0`. An all-None entry omits `cost` entirely (cost itself is optional). History: an earlier revision divided by `1e6` believing pi wanted $/token — that silently produced 1,000,000×-too-small costs AND schema-rejecting files (`must have required properties cacheWrite`); pi got past config load only to fail at every session start |
| opencode | `~/.config/opencode/opencode.jsonc` | set `provider.<id>` fragment + top-level `model`; json5-tolerant read; other keys preserved |
| claude | `~/.claude/settings.json` | set `env.ANTHROPIC_{BASE_URL,AUTH_TOKEN|API_KEY,MODEL,DEFAULT_{HAIKU,SONNET,OPUS}_MODEL}` from the **current preset**; **`ANTHROPIC_BASE_URL` = provider `baseUrl`** (R1, no override block); **null override deletes the stale key**; permissions/hooks/unrelated env preserved. claude binary may be absent (write anyway) |
| codex | `~/.codex/config.toml` + `auth.json` | set `model_provider="aio"` + `model` + `[model_providers.aio]` (`base_url` origin-normalized `/v1`, `wire_api`, `requires_openai_auth`) from the **current preset**; `auth.json` gets `OPENAI_API_KEY`. **auth.json written first; rollback on config.toml failure** |

Protocol compatibility per preset: claude needs `api==anthropic-messages` (R1 removed the anthropic-block escape hatch); codex needs non-anthropic. The UI filters (per-model `api` may override a model's protocol); apply is best-effort.

## Live config management — read, edit, delete, sync (08-27-agent-tabs-live-config)

The live channel is how the UI absorbs hand-edits made in the agent's NATIVE files. It is NOT a second config source — canonical stays SSOT. Only pi/opencode (`incremental_agent()` rejects claude/codex with 400: they manage presets in the canonical config).

- **Live readback** (`GET /api/models/agents` → `agents.<agent>.live`) now carries a `providers[]` summary next to the default readback. pi reads `settings.json` (default) and `models.json` (providers) **independently** — one corrupt file degrades to its half, live is null only when both fail. opencode reads `opencode.jsonc` (json5-tolerant) and reverse-infers `api` from the fragment's `npm` package (`@ai-sdk/anthropic`→`anthropic-messages`, else `openai-completions`). Summary items are `{id, name?, api?, baseUrl?, models: [ids]}`; malformed nodes are skipped, never fail the list.
- **edit/delete** (`render/{pi,opencode}.rs`) reuse the apply pipeline verbatim: key-level merge + `backup_write_verify_json` (rolling backups, atomic 0600, read-back, restore-on-failure). `ProviderPatch` is camelCase `{name?, baseUrl?, apiKey?, api?}`; `apiKey: ""` on the wire CLEARS the key (the frontend omits the field to keep it — blank input = keep). pi has no provider-node `name`, so patch.name is pi-ignored. opencode edit maps `api` back to the `npm` package (faithful inverse of the renderer). Deletes cascade the agent's dangling default: pi clears `defaultProvider`+`defaultModel` when they pointed at the deleted id; opencode clears top-level `model` when it starts `<id>/`. Sibling keys are never touched.
- **sync** (`POST .../sync`, body `{id?}`) imports native providers into the canonical library via the store's import adapters (`import_pi_providers` / `import_opencode_providers`, shared with `POST /api/models/import/pi`). Idempotent: ids already in canonical are reported in `skipped`, never overwritten. With `id`, the filter accepts **both the raw native key and the sanitized canonical id** — hand-written non-kebab keys (e.g. `My_Provider`) are visible in the live list under the raw key and must sync on click. opencode fragments lacking `options.baseURL` are un-importable and land in `skipped`; a single-id sync matching nothing is 404. Corrupt native file → 422, missing → 404.
- **Uninstalled agents**: live read returns null without error; the UI shows a prewrite-mode hint (apply pre-writes configs so a later install picks them up).

## Discover & test (`discover.rs`, `test.rs`)

- Discover: total 20s deadline across candidate URLs (primary derivation per `api`, then cc-switch fallbacks `/v1/models`, `/models`, anthropic-suffix-strip re-derive). First-candidate 401/403 short-circuits. Response parsing is multi-shape (bare array / `data|models|results|items` / object-of-objects), strips `models/` prefix, dedupes, natural sort. Errors: 502 with upstream body truncated 500 chars.
- Test: real minimal completion ("Reply with OK only.", `max_tokens:16`, no retries, 20s timeout). openai-completions→`/chat/completions`, openai-responses→`/responses`, anthropic-messages→`/v1/messages` (+`x-api-key`/`anthropic-version`). Success = 2xx. Returns `{ok, latency_ms, status, error?, response_text?}`.

## Catalog (`catalog.rs`, 08-27-provider-form-piweb)

- `GET /api/models/catalog` proxies `https://models.dev/api.json`, normalizes into `CatalogResponse { providers: [{ id, name, models: [{ id, name?, reasoning?, input?, contextWindow?, maxTokens?, cost? }] }] }`. Every field lookup is fallible (`Option`), never panics on upstream shape drift.
- **Cache + in-flight dedup**: `OnceLock<Mutex<Option<CatalogCache>>>`, 1h TTL. Unlike `usage.rs`'s cache (which releases the lock before the fresh compute), the catalog fetch happens **while holding the lock** — concurrent requests queue behind the in-flight fetch and land on the freshly-populated cache instead of firing their own request. No `?refresh=1` (models.dev changes slowly).
- 15s timeout, reuses `AppState.http` (the shared `reqwest::Client`, same as discover/test). Non-2xx or parse failure → 502 + upstream body truncated 500 chars (same contract as discover.rs).
- **cost fields pass through as-is** — models.dev's `cost.*` is already USD-per-1M-tokens, same unit as canonical; the pi render path is also pass-through ($/M), so no conversion happens anywhere in the chain.
- Frontend recommend matching (`types.ts::catalogRecommend`) is a **static host→models.dev-provider-id table** (`CATALOG_HOST_HINTS`) matched against the provider being edited's `baseUrl` hostname, then an exact (case-insensitive) model-id lookup within that catalog provider. The backend does not attempt host-based matching — it only normalizes and serves the flat catalog.

## Usage aggregation (`usage.rs`) — local session logs only

| agent | source | fields |
|---|---|---|
| pi | `~/.pi/agent/sessions/**/*.jsonl` | **`message.usage`** + `message.model` (NESTED under `message`, not record root) + record `timestamp` |
| opencode | `~/.local/share/opencode/opencode.db` (read-only SQLite) | `message.data` JSON: `tokens.{input,output,cache.{read,write}}` + `modelID`/`providerID` + `cost` + `time.created` |
| claude (when installed) | `~/.claude/projects/**/*.jsonl` | `message.usage.input_tokens/output_tokens/cache_creation_input_tokens/cache_read_input_tokens` + `message.model` |
| codex (when installed) | `~/.codex/sessions/**/*.jsonl` | `token_count` event `total_token_usage` |

- Window: today / 7d / all; cutoff injected into pure scan helpers. 30s process-global cache keyed by window; `?refresh=1` bypasses.
- Every record is fallible and skipped; one corrupt record never fails the response.
- **Zero-row filter (08-27-usage-correctness)**: after merging, the handler drops rows where
  `in+out+cacheRead+cacheWrite == 0` (noise rows like unused free models). Scan-level contracts
  unchanged; the filter lives at handler level (`rows.retain`, before `backfill_cost`).

### Cost backfill (`backfill_cost`, task 08-27-usage-correctness)

- **Unit convention**: canonical `provider.models[].cost` (in/out/cacheRead/cacheWrite) is
  **USD per 1M tokens** ($/M). Backfill formula: `Σ(tokens/1e6 × per_m)` per component —
  cache reads/writes use their own rates (cacheRead < input < cacheWrite), never the input rate.
- **Priority**: log cost > 0 → keep log value (never backfill over it — no double counting).
  Log cost == 0 or None → backfill from canonical by matching (agent, provider, model):
  a. row.provider known in canonical + exact model id with cost → use;
  b. cross-provider exact model-id match, first with cost → use (opencode's `providerID`
     never matches canonical, so it lands here);
  c. **version-suffix fuzzy only**: row model = canonical id + `-\d[\d.]*` (e.g. `-20250514`,
     `-4.5`); **letter-variant suffixes (`-free`, `-exp`) are different models and rejected**
     (`deepseek-v4-flash-free` must not bill at `deepseek-v4-flash` rates); reverse prefix
     matching (short row id vs longer canonical id) also rejected. Longest canonical id wins;
  d. no match → keep log value (0 stays 0 — a real free model; None stays None. Never invent).
- On a §b/§c hit with `row.provider == None` and a unique candidate provider, provider is
  backfilled too (claude/codex rows get a provider column value); ambiguous hits don't backfill.
- Verified against container logs: pi/opencode log cost fields exist but are **always 0**
  (untrustworthy) — hence the `> 0` trust threshold, not "field present".
- Frontend semantics: `hasCost = any(cost !== undefined)` gates the cost column;
  `hasCostValue = any(cost > 0)` gates the donut (all-zero draws no empty ring). Detail table
  splits Cache into Read/Write columns (`mcUsageColCacheR/W`); numeric columns right-aligned
  (`.ml-num`), sticky `thead`, `overflow-x` container, long names ellipsis + `title`.
  `ModelRow.tsx`'s expanded cost section carries the `($/M)` suffix (`mcCostPerM`, moved
  from the now-removed `ModelTable.tsx` — see the Frontend section below).

## Dependencies added

`reqwest` (json + rustls-tls — env-proxy aware, no openssl), `rusqlite` (bundled), `json5`. Shared `reqwest::Client` lives on `AppState` (per-request `.timeout()`).

## Frontend (`web/src/panes/models/` — split directory)

- New manifest pane type `"page"` (native in-app pane, `enabled` always true — no probe). Added to `ServiceType` in `web/src/types.ts`, `isServiceEntry`/`PaneForService` in `App.tsx`, "System" sidebar group, `serviceIcon` in `icons.tsx`.
- Module split (task 08-26-models-config-redesign): `index.tsx` re-exports `ModelsPane` (App.tsx imports `./panes/models`); `ModelsPane.tsx` owns ALL state + `/api/models/*` handlers; `types.ts` is the single decoder boundary (mirrors serde types); `ProviderGrid.tsx` (cc-switch style card grid) + `ProviderEditor.tsx` (right drawer: basic fields + advanced JSON + model list + binding overview + discover modal); `UsageTab.tsx` + `charts.tsx` (summary cards + token bar chart + cost donut, Kumo categorical palette, no chart dependency).
- **Model editor: `ModelRow.tsx` (08-27-provider-form-piweb, replaces the old flat `ModelTable.tsx`)** — pi-web style collapsed/expanded row per model. Collapsed: id input, display name, reasoning badge, cost summary (`in / out`), test button + pill, delete. Expanded: full field editor (protocol override, name, reasoning, contextWindow, maxTokens, cost 4-way grid) plus a "fill from models.dev" button. `ProviderEditor.tsx` renders `provider.models.map(m => <ModelRow .../>)` instead of a single table.
- **`ModelPicker.tsx`** — stateless model-id picker over one provider's model list (search + click-to-pick). No provider-selection layer of its own; wired into `AgentTabs.tsx` (08-27-agent-tabs-live-config) under a `.ml-model-trigger` button (selected shows `name (id)`): the agent-tab model field is pick-only over the chosen provider's `models[]` — no free text, canonical stays the only place model lists are edited.
- **models.dev fill** (`ModelsPane.tsx::handleCatalogFill` + `types.ts::catalogRecommend`): lazy-fetches `/api/models/catalog` once per pane session (cached in React state, not re-fetched per click). `catalogRecommend` matches the edited provider's `baseUrl` hostname against a static `CATALOG_HOST_HINTS` table to find the right catalog provider, then does an exact case-insensitive model-id lookup. On hit, overwrites `name/reasoning/contextWindow/maxTokens/cost` in one patch (full overwrite, not a merge — "fill" means "replace with the authoritative value"). On miss/error, `ModelRow` shows a `title`-and-inline hint (`mcCatalogNotFound` / `mcCatalogError`) without throwing.
- Agent tabs are split by semantics (08-27): `AgentTabs.tsx` serves the incremental agents (pi/opencode, single-assignment form); `PresetList.tsx` serves the switch-style agents (claude/codex) — cc-switch-style card list (name / protocol badge / current badge / live-vs-current match badge), row actions: set-current (switch = setCurrent + save + apply in one click), edit (inline form), duplicate (copy suffix, inserted after source), delete (current shifts to first remaining). Add/edit commit through the shared save bar (same PUT channel — no dedicated backend routes).
- **`LiveProviderList.tsx`** (08-27-agent-tabs-live-config) — the "live configs" section under an incremental agent tab: one collapsible row per provider node in the agent's native file (name/protocol badge/baseUrl/model count/current-default badge), row actions sync-to-library / field-level edit / delete (confirm). Read-only state from `GET /api/models/agents`; ALL mutations delegate up to `ModelsPane` (it owns every fetch) as `(agent, id, patch?)` callbacks. The inline edit form sends `LiveEditPatch` with `apiKey` omitted when blank (blank = keep — `""` on the wire would clear). Below the assignment form, `liveMatchState` (types.ts) derives a match/mismatch badge by comparing the live default against the canonical assignment (pi: provider+model; opencode: `<provider>/<model>` prefix). Uninstalled agent ⇒ prewrite hint instead of an error.
- Visual language: Kumo (`cloudflare_kumo_ui.md`) — `.ml-*` classes only, token-driven, card grid + layered surfaces; the old monolithic `mc-*` styles were removed.
- Agent tabs use the compat filter; apply result shows written/backup paths; not-installed shows an amber banner (config still prewritten).
- API responses decoded at ONE boundary (`types.ts` — typed interfaces mirror the serde types).
