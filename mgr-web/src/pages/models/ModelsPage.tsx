// ModelsPage — the mgr provider-library page, rewritten for the cc-switch
// refactor (task 10-04-model-config-cc-switch Phase 3, prd R1/D6).
//
// Structure: page-head → MgrNotice (explaining the mgr/sandbox split) →
// ProviderGrid (card wall) → ProviderEditorModal (center modal for both
// "new provider" and "edit provider"). There are no profile tabs, no agent
// tabs, no SandboxTable, no pi-import button, no models.dev catalog
// — those all died with the profile system (prd "废弃的功能").
//
// API contract (mgr/src/models.rs):
//   GET    /api/models/providers       — the masked provider library
//   PUT    /api/models/providers       — masked-echo merge + replace-all
//   POST   /api/models/providers       — add one provider
//   PUT    /api/models/providers/:id   — update one provider
//   DELETE /api/models/providers/:id   — remove one provider
//   POST   /api/models/discover        — provider probe
//   POST   /api/models/test            — minimal completion probe

import { useCallback, useEffect, useState } from "react";
import { ConfirmDialog } from "../../components/Dialogs";
import {
  createProvider,
  deleteProvider,
  discoverModels,
  getProviderLibrary,
  testModel,
  updateProvider,
} from "../../api";
import { t, type Lang } from "../../i18n";
import { MgrNotice } from "./MgrNotice";
import { ProviderEditorModal } from "./ProviderEditorModal";
import { ProviderGrid } from "./ProviderGrid";
import {
  deriveProviderIdFromName,
  emptyProvider,
  genProviderId,
  isMaskedKey,
  safeStringify,
  type DiscoverState,
  type ProviderLibrary,
  type ProviderTemplate,
  type TemplateCost,
  type TemplateModelEntry,
  type TestStateMap,
} from "./types";

/** The editor modal's working state: the id being edited (or null = closed)
 * plus the draft template. `isNew` distinguishes "add provider" from "edit
 * provider" (the id field is editable only when isNew). */
interface EditorState {
  id: string;
  draft: ProviderTemplate;
  isNew: boolean;
}

export function ModelsPage({ lang }: { lang: Lang }): JSX.Element {
  const [library, setLibrary] = useState<ProviderLibrary | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState("");

  // Editor modal state.
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveMsg, setSaveMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [headersText, setHeadersText] = useState("{}");
  const [showAdvanced, setShowAdvanced] = useState(false);

  // Delete-confirm dialog (a separate ConfirmDialog on top of the editor).
  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);

  // Per-(provider,model) test pills + discover modal state.
  const [testState, setTestState] = useState<TestStateMap>({});
  const [discover, setDiscover] = useState<DiscoverState | null>(null);

  // ── load ──────────────────────────────────────────────────────────

  const reload = useCallback(async () => {
    try {
      const lib = await getProviderLibrary();
      setLibrary(lib);
      setLoadError("");
    } catch (e) {
      setLoadError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    reload();
  }, [reload]);

  // ── editor open/close ─────────────────────────────────────────────

  const openEditor = (id: string, template: ProviderTemplate, isNew: boolean) => {
    setEditor({ id, draft: structuredClone(template), isNew });
    setHeadersText(safeStringify(template.headers, "{}"));
    setDirty(false);
    setSaveMsg(null);
    setShowAdvanced(false);
    setTestState({});
  };

  const closeEditor = () => {
    setEditor(null);
    setDiscover(null);
  };

  const onSelectProvider = (id: string) => {
    if (!library) return;
    const template = library.providers[id];
    if (!template) return;
    openEditor(id, template, false);
  };

  const onAddProvider = () => {
    if (!library) return;
    const id = genProviderId(library.providers);
    openEditor(id, emptyProvider(), true);
  };

  // ── draft patching ────────────────────────────────────────────────

  const patchDraft = (patch: Partial<ProviderTemplate>) => {
    if (!editor) return;
    setEditor({ ...editor, draft: { ...editor.draft, ...patch } });
    setDirty(true);
    // If the id is still the auto-generated placeholder, derive a meaningful
    // one from the display name (same heuristic as the workbench).
    if (patch.name !== undefined && editor.isNew) {
      const next = deriveProviderIdFromName(
        patch.name,
        library?.providers ?? {},
        editor.id,
      );
      if (next) setEditor((prev) => (prev ? { ...prev, id: next } : prev));
    }
  };

  const patchModel = (idx: number, patch: Partial<TemplateModelEntry>) => {
    if (!editor) return;
    const models = editor.draft.models.slice();
    models[idx] = { ...models[idx], ...patch };
    patchDraft({ models });
  };

  const addModel = () => {
    if (!editor) return;
    patchDraft({ models: [...editor.draft.models, { id: "" }] });
  };

  const deleteModel = (idx: number) => {
    if (!editor) return;
    patchDraft({ models: editor.draft.models.filter((_, i) => i !== idx) });
  };

  const updateCost = (idx: number, field: keyof TemplateCost, val: string) => {
    if (!editor) return;
    const num = val === "" ? undefined : parseFloat(val);
    const models = editor.draft.models.slice();
    const m = models[idx];
    models[idx] = {
      ...m,
      cost: { ...m.cost, [field]: num } as TemplateCost,
    };
    patchDraft({ models });
  };

  const onHeadersChange = (v: string) => {
    setHeadersText(v);
    // Validate + apply. Invalid JSON leaves the draft unchanged but still
    // marks the page dirty (the user is mid-edit).
    try {
      const parsed = JSON.parse(v) as Record<string, string>;
      patchDraft({ headers: parsed });
    } catch {
      setDirty(true);
    }
  };

  // ── save / delete ─────────────────────────────────────────────────

  const onSave = async () => {
    if (!editor) return;
    setSaving(true);
    setSaveMsg(null);
    try {
      // Apply the headers textarea if it parses (it might be dirty without
      // a successful patch).
      let headers = editor.draft.headers;
      try {
        headers = JSON.parse(headersText) as Record<string, string>;
      } catch {
        // keep the draft's last-good headers
      }
      const template = { ...editor.draft, headers };
      if (editor.isNew) {
        await createProvider(editor.id, template);
      } else {
        await updateProvider(editor.id, template);
      }
      setSaveMsg({ ok: true, text: t(lang, "mcSaved") });
      setDirty(false);
      await reload();
      closeEditor();
    } catch (e) {
      setSaveMsg({ ok: false, text: String(e) });
    } finally {
      setSaving(false);
    }
  };

  const onConfirmDelete = async () => {
    if (!deleteConfirmId) return;
    try {
      await deleteProvider(deleteConfirmId);
      setDeleteConfirmId(null);
      closeEditor();
      await reload();
    } catch (e) {
      setSaveMsg({ ok: false, text: String(e) });
      setDeleteConfirmId(null);
    }
  };

  // ── discover + test ───────────────────────────────────────────────

  const onFetchModels = async () => {
    if (!editor) return;
    setDiscover({
      loading: true,
      error: "",
      endpoint: "",
      models: [],
      filter: "",
      selected: new Set(),
    });
    try {
      // Probe split (follow-up #2): the draft's apiKey is the MASK when the
      // user hasn't retyped it (GET providers masks) — a masked Bearer is a
      // guaranteed 401. An untouched key on a SAVED provider probes ById so
      // the backend resolves the stored REAL key; a freshly typed key (or a
      // new provider) probes the literal draft fields.
      const body: Parameters<typeof discoverModels>[0] =
        !editor.isNew && isMaskedKey(editor.draft.apiKey)
          ? { providerId: editor.id }
          : {
              baseUrl: editor.draft.baseUrl,
              api: editor.draft.api,
              ...(editor.draft.apiKey
                ? { apiKey: editor.draft.apiKey }
                : {}),
            };
      const resp = await discoverModels(body);
      setDiscover({
        loading: false,
        error: "",
        endpoint: resp.endpoint,
        models: resp.models,
        filter: "",
        selected: new Set(),
      });
    } catch (e) {
      setDiscover({
        loading: false,
        error: String(e),
        endpoint: "",
        models: [],
        filter: "",
        selected: new Set(),
      });
    }
  };

  const onDiscoverAddSelected = () => {
    if (!editor || !discover) return;
    const existing = new Set(editor.draft.models.map((m) => m.id));
    const additions = [...discover.selected]
      .filter((id) => !existing.has(id))
      .map((id) => {
        const d = discover.models.find((m) => m.id === id);
        return { id, name: d?.name };
      });
    patchDraft({ models: [...editor.draft.models, ...additions] });
    setDiscover(null);
  };

  const onTest = async (modelId: string) => {
    if (!editor) return;
    const key = `${editor.id}:${modelId}`;
    setTestState((prev) => ({
      ...prev,
      [key]: { status: "testing" },
    }));
    const start = performance.now();
    try {
      // Same masked-key split as onFetchModels: untouched masked key on a
      // saved provider → ById (backend resolves the stored REAL key).
      const body: Parameters<typeof testModel>[0] =
        !editor.isNew && isMaskedKey(editor.draft.apiKey)
          ? { providerId: editor.id, modelId }
          : {
              baseUrl: editor.draft.baseUrl,
              api: editor.draft.api,
              ...(editor.draft.apiKey
                ? { apiKey: editor.draft.apiKey }
                : {}),
              modelId,
            };
      const resp = await testModel(body);
      const latencyMs = Math.round(performance.now() - start);
      setTestState((prev) => ({
        ...prev,
        [key]: resp.ok
          ? { status: "ok", latencyMs, responseText: resp.responseText }
          : {
              status: "fail",
              latencyMs,
              statusHttp: resp.status,
              error: resp.error ?? "unknown",
              responseText: resp.responseText,
            },
      }));
    } catch (e) {
      setTestState((prev) => ({
        ...prev,
        [key]: { status: "fail", error: String(e) },
      }));
    }
  };

  const onResetTest = (providerId: string, modelId: string) => {
    setTestState((prev) => {
      const next = { ...prev };
      delete next[`${providerId}:${modelId}`];
      return next;
    });
  };

  // ── render ────────────────────────────────────────────────────────

  if (loading) {
    return (
      <div className="page models-page">
        <p className="muted">{t(lang, "mcLoading")}</p>
      </div>
    );
  }
  if (loadError) {
    return (
      <div className="page models-page">
        <p className="ml-msg err">
          {t(lang, "mcLoadFailed")}
          {loadError}
        </p>
      </div>
    );
  }
  if (!library) {
    return (
      <div className="page models-page">
        <p className="muted">{t(lang, "mcLoadFailed")}</p>
      </div>
    );
  }

  return (
    <div className="page models-page">
      <div className="page-head">
        <h1>{t(lang, "mcProviders")}</h1>
        <p className="muted txs">{t(lang, "mcKeyHint")}</p>
      </div>

      <MgrNotice lang={lang} />

      {/* Always rendered, empty library included: the grid's trailing ghost
       * card IS the "new provider" entry, so an empty library must never
       * hide it (10-10-models-workspace-pane R1 - the old empty-state <p>
       * gated the only create entry behind already having one). */}
      <ProviderGrid
        library={library}
        onSelect={onSelectProvider}
        onAdd={onAddProvider}
        lang={lang}
      />

      {editor && (
        <ProviderEditorModal
          providerId={editor.id}
          provider={editor.draft}
          isNew={editor.isNew}
          dirty={dirty}
          saving={saving}
          saveMsg={saveMsg}
          headersText={headersText}
          showAdvanced={showAdvanced}
          testState={testState}
          discover={discover}
          onClose={closeEditor}
          onPatchProvider={patchDraft}
          onPatchModel={patchModel}
          onAddModel={addModel}
          onDeleteModel={deleteModel}
          onUpdateCost={updateCost}
          onHeadersChange={onHeadersChange}
          onToggleAdvanced={() => setShowAdvanced((v) => !v)}
          onSave={onSave}
          onTest={onTest}
          onResetTest={onResetTest}
          onFetchModels={onFetchModels}
          onDiscoverSet={setDiscover}
          onDiscoverAddSelected={onDiscoverAddSelected}
          onDeleteProvider={
            editor.isNew ? null : () => setDeleteConfirmId(editor.id)
          }
          lang={lang}
        />
      )}

      {deleteConfirmId && (
        <ConfirmDialog
          lang={lang}
          title={t(lang, "mcDeleteProvider")}
          desc={t(lang, "mcDeleteProviderConfirm")}
          danger
          onConfirm={onConfirmDelete}
          onCancel={() => setDeleteConfirmId(null)}
        />
      )}
    </div>
  );
}
