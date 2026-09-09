# Research: mgr-web frontend — structure, conventions, pages, bake path

- **Query**: mgr-web/src structure: App.tsx view union + nav, api.ts fetch pattern, types.ts conventions, i18n.ts structure, styles.css Kumo token system, existing pages inventory (SandboxListPage entry_url link, EditPage env form — where per-sandbox model-profile assignment UI slots in), build/bake into mgr image.
- **Scope**: internal
- **Date**: 2026-09-09

## 1. Package / build facts

- `mgr-web/package.json` — deps: react/react-dom 18.3.1, `@fontsource/inter ^5.1.0` only (:13-17). **No golden-layout, no xterm** (description: "no golden-layout - pure admin pages", :6 — this line changes under D1/D2).
- Build gate `mgr-web/package.json:9`: `build: "tsc --noEmit && vite build"`; `typecheck: "tsc --noEmit"`.
- `mgr-web/vite.config.ts:13-25` — `base: "/"`, outDir `dist`; dev proxy `/api` → `http://localhost:8089` (no auth, prd D9).
- `mgr-web/index.html` — pre-paint restore of `mgr.theme` / `mgr.lang` (mgr-prefixed keys; distinct from web/'s `aio.*` because mgr.localhost and the sandbox workbench are different origins, :11-19 comment).
- `mgr-web/src/main.tsx` — plain `createRoot(el).render(<App />)` (no StrictMode comment; no imperative lib yet — golden-layout port will need web/src/main.tsx:9-11's no-StrictMode rationale).
- Bake path: `mgr/Dockerfile:68-73` web-builder stage (node:20-bookworm-slim; `COPY mgr-web/package*.json` → `npm ci` → `COPY mgr-web/` → `npm run build`); `mgr/Dockerfile:102` `COPY --from=web-builder /mgr-web/dist /app/static`; `ENV MGR_WEB_DIR=/app/static` (:107). mgr-api serves it (main.rs:99-104). Adding golden-layout/xterm deps only changes package.json/lock — the Dockerfile stage is generic.

## 2. App.tsx — view union + nav

- Page union: `type Page = "sandboxes" | "images" | "models" | "usage"` (`mgr-web/src/App.tsx:37`).
- Sandboxes sub-view union `SbxView` (:41-47): `{view: "list"} | {view: "create"} | {view: "adopt"} | {view: "edit"; name} | {view: "job"; jobId; flow: "create"|"recreate"|"delete"}`.
- NO router library; page state in `useState` (module comment :1-9; spec frontend/directory-structure.md:35-36 "无路由库"). A workspace view (D2) extends this union (e.g. `page: "workspace"` + selected sandbox binding state) or gets its own top-level union member.
- State: theme/lang with `mgr.theme`/`mgr.lang` keys (:48-49, :54-73), scenario catalog fetched once and hoisted (:61, passed to Create/Edit via props).
- Sidebar nav (:83-149): four `launch-btn` items with `active` class (Sandboxes/Images/Models/Usage); footer theme + lang icon buttons. A workspace entry adds a fifth nav item; the per-tab sandbox binding (D2) lives in the workspace page, not in App (App keeps global chrome).
- Dispatcher `SandboxesPage` (:173-236) switches on the SbxView union.

## 3. api.ts — typed fetch client pattern

- Module owns ALL fetch/JSON decode (`mgr-web/src/api.ts:1-5`); pages never fetch directly (contrast: web/src does fetches in App.tsx).
- Two primitives (:34-48): `get<T>(path)` and `send<T>(path, method, body?)` — both throw `Error(await apiError(r))` on !ok, return `r.json() as T`.
- `apiError(r)` (types.ts:165-173): parse `{"error": ...}` JSON; fallback `HTTP <status>`.
- Functions: listScenarios (:54), listSandboxes/getSandbox/createSandbox/adoptSandbox/putSandbox/deleteSandbox/sandboxAction (:60-95), listImages/getJob (:99-105), getModelsConfig/putModelsConfig/importPiModels/discoverModels/testModel/getModelsCatalog/getUsage (:115-145). Models payloads decode through `pages/models/types.ts` decoders (`decodeConfig`, `decodeCatalog`, `decodeUsageFanout`) — `as` casts only inside api.ts + types modules.
- Per-sandbox proxied endpoints (D6/D7: term WS, buttons CRUD, manifest, stats) would join here — note the WS pane cannot use fetch; XtermPane constructs its own WebSocket URL (web/src/panes/XtermPane.tsx:25-28 pattern to port with a per-sandbox path).

## 4. types.ts — conventions

- Header (:1-19): single owner of /api/* shapes; backend owner map comment (routes.rs sandbox_json etc.); mirrors mgr/src/*.rs exactly.
- Key types: `Scenario` (:22-34, incl. always_on semantics), `SandboxEnv {scenarios, versions}` (:46-49 — never contains always_on ids), `Sandbox` (:58-73; `status` DB intent vs `live` compose fact; `entry_url`/`piweb_url` inline), `CreateBody`/`PutBody` (:81-97; PUT three-state limits documented at :88-92), `JobReply` (:100), `AdoptBody`/`AdoptReply` (:110-125), `DeleteReply` polymorphic + `isJobReply` (:131-135), `Image`/`ImageList` (:139-149), `Job` (:154-161).
- **PUT three-state contract text** (types.ts:88-92): absent=keep, >0=set, 0=clear — a `model_profile` assignment field would follow the same style documentation.
- A per-sandbox model-profile field on `Sandbox` + `PutBody` is where D8's UI contract lands (backend `sandbox_json` routes.rs:441-459 is the mirror).

## 5. i18n.ts

- Same pattern as web/src/i18n.ts: flat zh-CN/en table, `t()` + `fmt()`; 489 lines.
- Nav keys (:21-24): navSandboxes/navImages/navModels/navUsage; models page (:126 modelsSub); usage (:222-226 usageSub/muAll/muColSandbox/muNoSandboxes/muSandboxErr); mgr notice (`maMgrNotice` :132) for the models agent tabs.
- The workbench keys (golden-layout panes, sidebar groups, statusbar) currently exist ONLY in web/src/i18n.ts — porting the workspace (D2/D3/D5/D6) brings that key block over.

## 6. styles.css — Kumo token system

- Header (:1-9): "Token layer + shared control styles are ported from web/src/styles.css (official sources: kumo-ui.com + cloudflare/kumo @ 0c583257, MIT) with the golden-layout / pane / terminal / models-pane sections dropped — mgr-web is pure admin pages. Choose tokens by role, never by hue: light/dark stay synchronized through [data-mode] overrides only."
- Same token set as web/src/styles.css (bg/surface/surface-warm/fg/muted/border/border-soft/accent family/success/warn/danger/fonts/text sizes/space/radius/elev/focus-ring/motion + scrollbar) — light at `:root`, dark via `[data-mode=dark]`.
- **Missing vs web/**: the `--term-*` tokens and `.pane`/`.gl-*`/golden-layout overrides — the port needs web/src/gl-kumo.css (278 lines) + the trimmed styles.css sections (pane, term, sidebar/statusbar layout of the workbench shell) re-imported. 2249 lines currently.

## 7. Existing pages inventory

| Page | File | Notes |
|---|---|---|
| SandboxListPage | pages/SandboxListPage.tsx (358) | 4s auto-poll (:27, :55-59); card with StatusBadges (DB intent + live, :314-358); actions start/stop/restart (disabled by live state :272-293), edit (native only :294-299), delete confirm + volumes checkbox (:147-190); **entry link** `<a href={sb.entry_url} target="_blank">` (:219-229, i18n `enterNewTab` "在新标签页打开该沙箱的工作台") — under D2 this becomes "open workspace tab" and the href/onClick changes |
| CreatePage | pages/CreatePage.tsx (178) | name slug `NAME_RE = /^[a-z0-9][a-z0-9-]{0,31}$/` (:31, mirrors routes.rs validate_name); EnvPicker + resources; POST → JobView |
| AdoptPage | pages/AdoptPage.tsx (157) | register external compose stack (Phase 5) |
| EditPage | pages/EditPage.tsx (195) | env editor; see §8 |
| JobView | pages/JobView.tsx (156) | 1.5s poll (`POLL_MS = 1500` :17); log tail auto-scroll; flow-typed headings |
| ImagesPage | pages/ImagesPage.tsx (96) | env-hash registry; expandable build_log; no delete (design defers to host docker rmi) |
| ModelsPage | pages/models/ModelsPage.tsx (859) | port of web ModelsPane; tabs providers/pi/opencode/claude/codex (:61-63); MgrNotice links to sandbox entry_urls (:116-161, :770/786) — these become workspace links under D2 |
| models sub-components | ProviderGrid 150 / ProviderEditor 494 / PresetList 455 / AgentTabs 159 / ModelPicker 57 / ModelRow 250 / MgrNotice 41 / charts 146 / types 550+ | trimmed ports (no sandbox-local agent APIs) |
| UsagePage | pages/UsagePage.tsx (353) | GET /api/usage fan-out; 30s poll; sandbox chips + combined view; summarize() ported math (:301-353) |

## 8. EditPage — env form fields + where model-profile assignment UI slots in

`mgr-web/src/pages/EditPage.tsx`:
- Loads `getSandbox(name)` + scenario catalog in parallel (:48-75); seeds `env`, `origEnv`, cpus/mem (:59-68).
- Form layout (`wizard` div): EnvPicker (:140-144) → resources label (:146-149) → `field-row` with cpus + memMb inputs (:150-172) → dialog-actions with save (:174-183).
- **PUT body construction (:80-83, :103-107)**: `cpusOut/memOut` computed per the three-state contract (emptied field sends 0 to CLEAR); `putSandbox(name, { env, cpus: cpusOut, mem_mb: memOut })` → `onSubmitted(r.job)` → JobView (recreate flow).
- `changed` guard (:89-94) compares canonicalized env + resource values — a profile field joins this comparison.
- **Where D8's per-sandbox model-profile assignment slots in**: a new field/row between the resources block (:146-172) and dialog-actions (:174) — e.g. a `<select>` of profiles (fetched like scenarios, hoisted or local) whose value flows into the PUT body as `model_profile`; the backend mirror is `PutBody` (mgr-web/src/types.ts:93-97) + routes.rs `PutBody` (:485-490) + put_sandbox merge (:504-521), and — since profile assignment probably should NOT trigger a full recreate job (unlike env, which rebuilds images) — either the PUT handler grows a non-recreating fast path or a separate assignment endpoint is used (routes.rs new route or models.rs profile routes).
- Adopted sandboxes never reach this page (:9-10 header; card hides the button).

## 9. Golden-layout port surface (D2) — what must be added

From web/ (see research/workbench-frontend.md): golden-layout ^2.6.0 + @xterm/xterm ^5.5.0 + @xterm/addon-fit deps; `golden-layout/dist/css/goldenlayout-base.css` + a gl-kumo.css port; the App-level workspace component (single factory registration, React-root-per-container, beforeComponentRelease cleanup, localStorage layout persist under e.g. `mgr.layout`, popout child via `gl-window` param + `window.__glInstance`, iframe drag-capture class, tab glyph patching); XtermPane with per-sandbox WS URL; IframePane with per-sandbox URL computation. mgr-web/src/main.tsx may need the no-StrictMode guard (web/src/main.tsx:9-11 rationale).

## 10. Caveats / Not Found

- mgr-web/src/icons.tsx not deep-read — it carries the sprite pattern (same as web/src/icons.tsx) plus box/chart icons; the workspace port adds the service glyphs (web serviceIcon map, web/src/icons.tsx:90-105).
- No router, no state library, no test harness in mgr-web (no smoke-test.cjs equivalent) — verification story for the workspace page is manual/verify-skill based.
- `mgr-web/src/pages/models/types.ts` is ~550 lines; only the head (types through TestResponse) was quoted in this research — decoders `decodeUsageFanout`/`decodeConfig`/`decodeCatalog` + helpers (catalogRecommend, rebindAgentProviders, incompatibleReason, etc.) mirror web's; consult directly when editing.
- mgr-web has zero WebSocket usage today — the xterm pane is its first WS consumer.
