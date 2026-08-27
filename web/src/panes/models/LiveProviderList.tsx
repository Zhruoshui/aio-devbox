// LiveProviderList — the "live configs" section of an incremental agent tab
// (pi/opencode, 08-27-agent-tabs-live-config). Renders every provider node
// found in the agent's NATIVE config file with per-row actions:
// - sync into the canonical library (idempotent; skipped when already there)
// - field-level edit (provider-level fields only — per-model editing goes
//   through the library, design §6)
// - delete (dangling default cleanup happens backend-side)
// The list is read-only state from GET /api/models/agents live.providers;
// mutations are delegated upward so ModelsPane owns every fetch.

import { useState } from "react";
import { Icon } from "../../icons";
import { t, type Lang } from "../../i18n";
import {
  API_PROTOCOLS,
  isLiveDefault,
  protocolLabel,
  type AgentLive,
  type LiveProviderSummary,
} from "./types";

/** Patch sent to PUT /api/models/agents/:agent/provider/:id. apiKey "" would
 * mean "clear" on the wire, so an untouched key is simply omitted. */
export interface LiveEditPatch {
  name?: string;
  baseUrl?: string;
  apiKey?: string;
  api?: string;
}

export function LiveProviderList({
  agent,
  live,
  busyId,
  onSync,
  onEdit,
  onDelete,
  lang,
}: {
  agent: "pi" | "opencode";
  live: AgentLive | null;
  /** Row-level busy marker (sync/edit/delete in flight for that row). */
  busyId: string | null;
  onSync: (id: string) => void;
  onEdit: (id: string, patch: LiveEditPatch) => void;
  onDelete: (id: string) => void;
  lang: Lang;
}): JSX.Element {
  const [expanded, setExpanded] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [form, setForm] = useState<LiveEditPatch>({});

  const providers = live?.providers ?? [];

  if (providers.length === 0) {
    return <div className="ml-empty">{t(lang, "maLiveEmpty")}</div>;
  }

  const openEdit = (p: LiveProviderSummary) => {
    setEditing(p.id);
    setExpanded(p.id);
    setForm({
      name: p.name ?? "",
      baseUrl: p.baseUrl ?? "",
      apiKey: "", // blank = keep the stored key (mcApiKeyPh)
      api: p.api ?? "openai-completions",
    });
  };

  return (
    <div className="ml-live-list">
      {providers.map((p) => {
        const isDefault = isLiveDefault(agent, live, p.id);
        const isOpen = expanded === p.id;
        const isEditing = editing === p.id;
        return (
          <div key={p.id} className="ml-live-row">
            <div className="ml-live-row-head">
              <button
                className="icon-btn ml-live-expand"
                aria-label={isOpen ? t(lang, "mcCollapse") : t(lang, "mcExpand")}
                title={p.id}
                onClick={() => setExpanded(isOpen ? null : p.id)}
              >
                <Icon name={isOpen ? "chev-r" : "chev-l"} />
              </button>
              <span className="ml-live-name" title={p.id}>
                {p.name || p.id}
              </span>
              {p.api && (
                <span className="ml-badge ml-badge-protocol">
                  {protocolLabel(p.api)}
                </span>
              )}
              {p.baseUrl && (
                <span className="ml-live-url" title={p.baseUrl}>
                  {p.baseUrl}
                </span>
              )}
              <span className="ml-live-count">
                {p.models.length} {t(lang, "maModelsSuffix")}
              </span>
              {isDefault && (
                <span className="ml-badge ml-badge-current">
                  {t(lang, "maLiveCurrent")}
                </span>
              )}
              <span className="ml-live-actions">
                <button
                  className="btn btn-secondary btn-sm"
                  disabled={busyId === p.id}
                  onClick={() => onSync(p.id)}
                >
                  {busyId === p.id ? <Icon name="refresh" /> : null}
                  {t(lang, "maSyncToLib")}
                </button>
                <button
                  className="btn btn-secondary btn-sm"
                  disabled={busyId === p.id}
                  onClick={() => (isEditing ? setEditing(null) : openEdit(p))}
                >
                  {t(lang, "mcEdit")}
                </button>
                <button
                  className="btn btn-secondary btn-sm"
                  disabled={busyId === p.id}
                  onClick={() => {
                    if (window.confirm(t(lang, "maConfirmDeleteLive"))) {
                      onDelete(p.id);
                    }
                  }}
                >
                  {t(lang, "mcDeleteProvider")}
                </button>
              </span>
            </div>

            {/* expanded: model id chips */}
            {isOpen && !isEditing && (
              <div className="ml-live-models">
                {p.models.length === 0 ? (
                  <span className="ml-hint">—</span>
                ) : (
                  p.models.map((m) => (
                    <code key={m} className="ml-live-model-chip">
                      {m}
                    </code>
                  ))
                )}
              </div>
            )}

            {/* inline edit form (field-level patch, design §2) */}
            {isEditing && (
              <div className="ml-live-edit">
                <div className="field">
                  <label>{t(lang, "mcName")}</label>
                  <input
                    value={form.name ?? ""}
                    onChange={(e) => setForm({ ...form, name: e.target.value })}
                  />
                </div>
                <div className="field">
                  <label>{t(lang, "mcBaseUrl")}</label>
                  <input
                    value={form.baseUrl ?? ""}
                    onChange={(e) => setForm({ ...form, baseUrl: e.target.value })}
                  />
                </div>
                <div className="field">
                  <label>{t(lang, "mcApi")}</label>
                  <select
                    value={form.api ?? "openai-completions"}
                    onChange={(e) => setForm({ ...form, api: e.target.value })}
                  >
                    {API_PROTOCOLS.map((a) => (
                      <option key={a} value={a}>
                        {protocolLabel(a)}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="field">
                  <label>{t(lang, "mcApiKey")}</label>
                  <input
                    type="password"
                    placeholder={t(lang, "mcApiKeyPh")}
                    value={form.apiKey ?? ""}
                    onChange={(e) => setForm({ ...form, apiKey: e.target.value })}
                  />
                </div>
                <div className="ml-live-edit-actions">
                  <button
                    className="btn btn-primary btn-sm"
                    disabled={busyId === p.id}
                    onClick={() => {
                      const patch: LiveEditPatch = {
                        name: form.name,
                        baseUrl: form.baseUrl,
                        api: form.api,
                      };
                      // "" on the wire would CLEAR the key — only send when
                      // the user typed something.
                      if (form.apiKey) patch.apiKey = form.apiKey;
                      onEdit(p.id, patch);
                      setEditing(null);
                    }}
                  >
                    {t(lang, "maSaveLiveEdit")}
                  </button>
                  <button
                    className="btn btn-secondary btn-sm"
                    onClick={() => setEditing(null)}
                  >
                    {t(lang, "maCancelLiveEdit")}
                  </button>
                </div>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
