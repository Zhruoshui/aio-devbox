// StorePage — the usertools tool store (10-20-usertools-app-store).
//
// Replaces the retired single-file editor (UserToolsPage): the per-tool dirs
// under usertools/ are the source of truth, this page is the shelf.
//   * 我的货架 — materialized tools (cards: category chip, version, install
//     type, reference count, edit/remove).
//   * 推荐货架 — the builtin seed list (apt-only); one click materializes.
//   * 上架 — a typed recipe form (apt / URL / GitHub Release / script /
//     custom fragment) with a SERVER-rendered live preview (the backend
//     generator is the single owner of the fragment templates).
//   * Raw channel — legacy fragment-only tools edit their fragment verbatim
//     (the old textarea interactions: Tab indent, status line).
// Install stays where it always was: the create/edit wizard's L5 layer
// (mgr never triggers rebuilds).

import { useCallback, useEffect, useRef, useState } from "react";

import {
  createToolFromBuiltin,
  createToolFromRecipe,
  deleteTool,
  getToolFragment,
  getToolStore,
  previewTool,
  putToolFragment,
  updateTool,
} from "../api";
import { t, type Lang, type StringKey } from "../i18n";
import {
  TOOL_CATEGORIES,
  type Install,
  type InstallType,
  type Recipe,
  type ToolCard,
  type UrlKind,
} from "../types";

/** install_type → form-type vocabulary ("custom" never appears in the form). */
type FormType = Exclude<Install["type"], never>;
const FORM_TYPES: FormType[] = ["apt", "url", "github-release", "script", "raw"];

/** Card wire vocabulary (Install::label() + "custom"). NOTE: a GitHub
 * Release recipe surfaces as url-binary/url-tarball — the download KIND is
 * the shelf-relevant fact; the form keeps its own richer selector below. */
const TYPE_KEYS: Record<InstallType, StringKey> = {
  apt: "stTypeApt",
  "url-binary": "stTypeUrl",
  "url-tarball": "stTypeUrl",
  script: "stTypeScript",
  raw: "stTypeRaw",
  custom: "stCustomFrag",
};

/** The form's five install types (Install["type"] as authored). */
const FORM_TYPE_KEYS: Record<FormType, StringKey> = {
  apt: "stTypeApt",
  url: "stTypeUrl",
  "github-release": "stTypeGithub",
  script: "stTypeScript",
  raw: "stTypeRaw",
};

/** The form's working copy: everything as editable strings; assembled into
 * a Recipe on preview/save (one source of truth for both). */
interface FormState {
  name: string;
  description: string;
  category: string;
  version: string;
  homepage: string;
  installType: FormType;
  packages: string;
  url: string;
  urlKind: UrlKind;
  bin: string;
  binsText: string;
  repo: string;
  tag: string;
  asset: string;
  script: string;
  check: string;
  raw: string;
}

const emptyForm = (): FormState => ({
  name: "",
  description: "",
  category: "other",
  version: "",
  homepage: "",
  installType: "apt",
  packages: "",
  url: "",
  urlKind: "binary",
  bin: "",
  binsText: "",
  repo: "",
  tag: "latest",
  asset: "",
  script: "",
  check: "",
  raw: "",
});

const lines = (text: string): string[] =>
  text.split("\n").map((l) => l.trim()).filter(Boolean);

/** Assemble the Recipe the preview/save both send — the ONE conversion, so
 * the preview can never lie about what the save writes. */
function toRecipe(f: FormState): Recipe {
  const meta = {
    schema: 1,
    name: f.name.trim(),
    description: f.description.trim(),
    category: f.category,
    version: f.version.trim(),
    homepage: f.homepage.trim(),
  };
  let install: Install;
  switch (f.installType) {
    case "apt":
      install = { type: "apt", packages: f.packages.trim().split(/\s+/).filter(Boolean) };
      break;
    case "url":
      install = {
        type: "url",
        url: f.url.trim(),
        kind: f.urlKind,
        bins: f.urlKind === "binary" ? (f.bin.trim() ? [f.bin.trim()] : []) : lines(f.binsText),
      };
      break;
    case "github-release":
      install = {
        type: "github-release",
        repo: f.repo.trim(),
        tag: f.tag.trim(),
        asset: f.asset.trim(),
        kind: f.urlKind,
        bins: f.urlKind === "binary" ? (f.bin.trim() ? [f.bin.trim()] : []) : lines(f.binsText),
      };
      break;
    case "script":
      install = { type: "script", script: f.script, check: f.check.trim() || null };
      break;
    case "raw":
      install = { type: "raw", dockerfile: f.raw };
      break;
  }
  return { ...meta, install };
}

/** Pre-fill the form from an existing recipe (edit flow). */
function fromRecipe(r: Recipe): FormState {
  const f = emptyForm();
  f.name = r.name;
  f.description = r.description;
  f.category = r.category;
  f.version = r.version;
  f.homepage = r.homepage;
  const kind: UrlKind = "kind" in r.install ? r.install.kind : "binary";
  switch (r.install.type) {
    case "apt":
      f.installType = "apt";
      f.packages = r.install.packages.join(" ");
      break;
    case "url":
      f.installType = "url";
      f.url = r.install.url;
      f.urlKind = kind;
      f.bin = r.install.bins[0] ?? "";
      f.binsText = r.install.bins.join("\n");
      break;
    case "github-release":
      f.installType = "github-release";
      f.repo = r.install.repo;
      f.tag = r.install.tag;
      f.asset = r.install.asset;
      f.urlKind = kind;
      f.bin = r.install.bins[0] ?? "";
      f.binsText = r.install.bins.join("\n");
      break;
    case "script":
      f.installType = "script";
      f.script = r.install.script;
      f.check = r.install.check ?? "";
      break;
    case "raw":
      f.installType = "raw";
      f.raw = r.install.dockerfile;
      break;
  }
  return f;
}

// ── main page ─────────────────────────────────────────────────────

export function StorePage({ lang }: { lang: Lang }): JSX.Element {
  const [shelf, setShelf] = useState<{ mine: ToolCard[]; builtin: ToolCard[] } | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  // Dialogs: recipe editor (create/edit), raw editor, delete confirm.
  const [editId, setEditId] = useState<string | null | undefined>(undefined); // undefined = closed, null = create
  const [rawId, setRawId] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<ToolCard | null>(null);

  const load = useCallback(async () => {
    try {
      setShelf(await getToolStore());
      setError("");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const addBuiltin = async (id: string): Promise<void> => {
    setBusy(true);
    try {
      await createToolFromBuiltin(id);
      await load();
      setError("");
      setNotice(t(lang, "stAddedNotice"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (card: ToolCard, force: boolean): Promise<void> => {
    setBusy(true);
    try {
      await deleteTool(card.id, force);
      setDeleting(null);
      await load();
      setError("");
      setNotice(t(lang, "stDeletedNotice"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  /** Edit button routing: recipe tools → form, legacy tools → raw editor. */
  const openEdit = async (card: ToolCard): Promise<void> => {
    if (!card.has_recipe) {
      setRawId(card.id);
      return;
    }
    setEditId(card.id); // RecipeDialog loads the recipe itself
  };

  const onSaved = (referenced: boolean): void => {
    setEditId(undefined);
    setNotice(t(lang, referenced ? "stEditNotice" : "stSavedNotice"));
  };

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>{t(lang, "navToolStore")}</h1>
          <p className="sub">{t(lang, "stSub")}</p>
        </div>
        <div className="page-actions">
          <button className="btn btn-secondary" disabled={busy} onClick={() => void load()}>
            ⟳ {t(lang, "stRefresh")}
          </button>
          <button className="btn btn-primary" onClick={() => setEditId(null)}>
            {t(lang, "stPublish")}
          </button>
        </div>
      </div>

      {error && <div className="status error">{error}</div>}
      {notice && !error && <div className="status ok">{notice}</div>}

      <section className="st-shelf">
        <h2>{t(lang, "stMyShelf")}</h2>
        {shelf?.mine.length === 0 && <p className="st-hint">{t(lang, "stEmptyMine")}</p>}
        <div className="st-grid">
          {shelf?.mine.map((c) => (
            <ToolCardView
              key={c.id}
              card={c}
              lang={lang}
              busy={busy}
              onEdit={() => void openEdit(c)}
              onDelete={() => setDeleting(c)}
            />
          ))}
        </div>
      </section>

      <section className="st-shelf">
        <h2>{t(lang, "stBuiltinShelf")}</h2>
        <div className="st-grid">
          {shelf?.builtin.map((c) => (
            <ToolCardView
              key={c.id}
              card={c}
              lang={lang}
              busy={busy}
              builtin
              onAdd={() => void addBuiltin(c.id)}
            />
          ))}
        </div>
      </section>

      {editId !== undefined && (
        <RecipeDialog
          lang={lang}
          id={editId}
          onClose={() => setEditId(undefined)}
          onSaved={onSaved}
        />
      )}
      {rawId !== null && (
        <RawFragmentDialog
          lang={lang}
          id={rawId}
          onClose={() => setRawId(null)}
          onSaved={() => {
            setRawId(null);
            setNotice(t(lang, "stSavedNotice"));
            void load();
          }}
        />
      )}
      {deleting !== null && (
        <DeleteDialog lang={lang} card={deleting} busy={busy} onConfirm={remove} onClose={() => setDeleting(null)} />
      )}
    </div>
  );
}

// ── card ──────────────────────────────────────────────────────────

function ToolCardView({
  card,
  lang,
  busy,
  builtin = false,
  onEdit,
  onDelete,
  onAdd,
}: {
  card: ToolCard;
  lang: Lang;
  busy: boolean;
  builtin?: boolean;
  onEdit?: () => void;
  onDelete?: () => void;
  onAdd?: () => void;
}): JSX.Element {
  const refs = card.referenced_by.length;
  const refText =
    refs === 0
      ? t(lang, "stNotReferenced")
      : refs === 1
        ? t(lang, "stRefsOne")
        : t(lang, "stRefsMany").replace("{n}", String(refs));
  return (
    <div className={`st-card st-cat-${card.category}`}>
      <div className="st-card-head">
        <span className="st-cat">{card.category}</span>
        <span className="st-type">{t(lang, TYPE_KEYS[card.install_type])}</span>
      </div>
      <div className="st-name">
        {card.homepage ? (
          <a href={card.homepage} target="_blank" rel="noreferrer">
            {card.name}
          </a>
        ) : (
          card.name
        )}
        {!card.has_recipe && !builtin && <span className="st-badge">{t(lang, "stCustomFrag")}</span>}
      </div>
      <div className="st-desc">{card.description || t(lang, "stNoDesc")}</div>
      <div className="st-meta">
        {card.version && <span className="st-version">{card.version}</span>}
        <span className={`st-refs${refs > 0 ? " on" : ""}`} title={card.referenced_by.join(", ")}>
          {refText}
        </span>
      </div>
      <div className="st-actions">
        {builtin ? (
          <button className="btn btn-secondary" disabled={busy} onClick={onAdd}>
            {t(lang, "stAdd")}
          </button>
        ) : (
          <>
            <button className="btn btn-secondary" onClick={onEdit}>
              {t(lang, card.has_recipe ? "stEdit" : "stEditFragment")}
            </button>
            <button className="btn btn-danger" disabled={busy} onClick={onDelete}>
              {t(lang, "stDelete")}
            </button>
          </>
        )}
      </div>
    </div>
  );
}

// ── recipe form dialog ────────────────────────────────────────────

function RecipeDialog({
  lang,
  id,
  onClose,
  onSaved,
}: {
  lang: Lang;
  /** null = create; a string = edit that tool (recipe loaded on mount). */
  id: string | null;
  onClose: () => void;
  onSaved: (referenced: boolean) => void;
}): JSX.Element {
  const [form, setForm] = useState<FormState>(emptyForm());
  const [loaded, setLoaded] = useState(id === null);
  const [referenced, setReferenced] = useState(false);
  const [preview, setPreview] = useState("");
  const [previewError, setPreviewError] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  // Edit flow: fetch the recipe (and the card's reference state rides the
  // store fetch — simpler: probe via the fragment endpoint's recipe).
  useEffect(() => {
    if (id === null) return;
    void (async () => {
      try {
        const d = await getToolFragment(id);
        if (d.recipe === null) {
          // Corrupt/legacy race — the card decided this was a form tool.
          setError(t(lang, "stCustomFrag"));
          return;
        }
        setForm(fromRecipe(d.recipe));
        setLoaded(true);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    })();
  }, [id, lang]);

  // Debounced server preview: the backend generator is the single owner of
  // the templates — this preview IS what the save will write.
  useEffect(() => {
    if (!loaded) return;
    const timer = setTimeout(() => {
      void (async () => {
        try {
          const r = await previewTool(id, toRecipe(form));
          setPreview(r.fragment);
          setPreviewError("");
        } catch (e) {
          setPreview("");
          setPreviewError(e instanceof Error ? e.message : String(e));
        }
      })();
    }, 400);
    return () => clearTimeout(timer);
  }, [form, id, loaded]);

  // The "next rebuild" notice needs to know whether the tool is referenced;
  // only meaningful in the edit flow.
  useEffect(() => {
    if (id === null) return;
    void (async () => {
      try {
        const shelf = await getToolStore();
        setReferenced(
          shelf.mine.some((c) => c.id === id && c.referenced_by.length > 0),
        );
      } catch {
        /* display-only */
      }
    })();
  }, [id]);

  const set = <K extends keyof FormState>(key: K, value: FormState[K]): void =>
    setForm((f) => ({ ...f, [key]: value }));

  const save = async (): Promise<void> => {
    setBusy(true);
    try {
      const recipe = toRecipe(form);
      if (id === null) await createToolFromRecipe(recipe);
      else await updateTool(id, recipe);
      onSaved(referenced);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const field = (
    key: keyof FormState,
    labelKey: StringKey,
    opts: { textarea?: boolean; mono?: boolean } = {},
  ): JSX.Element => (
    <label className="st-field">
      <span>{t(lang, labelKey as never)}</span>
      {opts.textarea ? (
        <textarea
          className={`input${opts.mono ? " mono" : ""}`}
          rows={opts.mono ? 8 : 3}
          spellCheck={false}
          value={form[key] as string}
          onChange={(e) => set(key, e.target.value as FormState[typeof key])}
        />
      ) : (
        <input
          className={`input${opts.mono ? " mono" : ""}`}
          value={form[key] as string}
          onChange={(e) => set(key, e.target.value as FormState[typeof key])}
        />
      )}
    </label>
  );

  return (
    <div className="overlay open" role="presentation" onClick={onClose}>
      <div
        className="dialog st-dialog"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="page-head">
          <h2>{t(lang, id === null ? "stFormTitleNew" : "stFormTitleEdit")}</h2>
          <button className="btn btn-secondary" onClick={onClose}>
            {t(lang, "stClose")}
          </button>
        </div>

        {error && <div className="status error">{error}</div>}

        <div className="st-form-grid">
          <div className="st-form-left">
            {field("name", "stFormName")}
            {field("description", "stFormDesc")}
            <label className="st-field">
              <span>{t(lang, "stFormCategory")}</span>
              <select
                className="input"
                value={form.category}
                onChange={(e) => set("category", e.target.value)}
              >
                {TOOL_CATEGORIES.map((c) => (
                  <option key={c} value={c}>
                    {c}
                  </option>
                ))}
              </select>
            </label>
            {field("version", "stFormVersion")}
            {field("homepage", "stFormHomepage")}

            <label className="st-field">
              <span>{t(lang, "stFormType")}</span>
              <select
                className="input"
                value={form.installType}
                onChange={(e) =>
                  setForm((f) => ({ ...f, installType: e.target.value as FormType }))
                }
              >
                {FORM_TYPES.map((ty) => (
                  <option key={ty} value={ty}>
                    {t(lang, FORM_TYPE_KEYS[ty])}
                  </option>
                ))}
              </select>
            </label>

            {form.installType === "apt" && field("packages", "stFormPackages")}

            {(form.installType === "url" || form.installType === "github-release") && (
              <>
                <label className="st-field">
                  <span>{t(lang, "stFormType")} · kind</span>
                  <select
                    className="input"
                    value={form.urlKind}
                    onChange={(e) => set("urlKind", e.target.value as UrlKind)}
                  >
                    <option value="binary">{t(lang, "stKindBinary")}</option>
                    <option value="tarball">{t(lang, "stKindTarball")}</option>
                  </select>
                </label>
                {form.installType === "url"
                  ? field("url", "stFormUrl", { mono: true })
                  : (
                      <>
                        {field("repo", "stFormRepo", { mono: true })}
                        {field("tag", "stFormTag", { mono: true })}
                        {field("asset", "stFormAsset", { mono: true })}
                      </>
                    )}
                {form.urlKind === "binary"
                  ? field("bin", "stFormBin", { mono: true })
                  : field("binsText", "stFormBins", { textarea: true, mono: true })}
              </>
            )}

            {form.installType === "script" && (
              <>
                {field("script", "stFormScript", { textarea: true, mono: true })}
                {field("check", "stFormCheck", { mono: true })}
              </>
            )}
            {form.installType === "raw" && field("raw", "stFormRaw", { textarea: true, mono: true })}
          </div>

          <div className="st-form-right">
            <div className="st-preview-title">{t(lang, "stPreview")}</div>
            {previewError ? (
              <div className="status error">{previewError}</div>
            ) : (
              <pre className="st-preview mono">{preview || t(lang, "stPreviewing")}</pre>
            )}
            <p className="st-hint">{t(lang, "stPreviewHint")}</p>
          </div>
        </div>

        <div className="page-actions">
          <button className="btn btn-primary" disabled={busy || !loaded} onClick={() => void save()}>
            {busy ? t(lang, "stSaving") : t(lang, "stSave")}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── raw fragment dialog (legacy tools) ────────────────────────────

function RawFragmentDialog({
  lang,
  id,
  onClose,
  onSaved,
}: {
  lang: Lang;
  id: string;
  onClose: () => void;
  onSaved: () => void;
}): JSX.Element {
  const [raw, setRaw] = useState("");
  const [saved, setSaved] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const taRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    void (async () => {
      try {
        const d = await getToolFragment(id);
        setRaw(d.content);
        setSaved(d.content);
        setError("");
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    })();
  }, [id]);

  const dirty = raw !== saved;

  /** Tab indents two spaces (textarea default moves focus) — carried over
   * from the retired UserToolsPage editor verbatim. */
  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>): void => {
    const ta = e.currentTarget;
    if (e.key !== "Tab") return;
    e.preventDefault();
    const { selectionStart: s, selectionEnd: ep, value } = ta;
    const next =
      value.slice(0, s) + "  " + value.slice(ep);
    setRaw(next);
    requestAnimationFrame(() => ta.setSelectionRange(s + 2, s + 2));
  };

  const save = async (): Promise<void> => {
    setBusy(true);
    try {
      await putToolFragment(id, raw);
      setSaved(raw);
      setError("");
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="overlay open" role="presentation" onClick={onClose}>
      <div
        className="dialog st-dialog"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="page-head">
          <h2>
            {t(lang, "stRawTitle")} · <code className="mono">{id}</code>
          </h2>
          <button className="btn btn-secondary" onClick={onClose}>
            {t(lang, "stClose")}
          </button>
        </div>
        {error && <div className="status error">{error}</div>}
        <p className="st-hint">{t(lang, "stRawHint")}</p>
        <textarea
          ref={taRef}
          className="input mono ut-textarea"
          spellCheck={false}
          value={raw}
          onKeyDown={onKeyDown}
          onChange={(e) => setRaw(e.target.value)}
        />
        <div className="st-meta mono">
          {t(lang, "stRawStatus")
            .replace("{lines}", String(raw === "" ? 0 : raw.split("\n").length))
            .replace("{state}", dirty ? t(lang, "stRawDirty") : t(lang, "stRawSaved"))}
        </div>
        <div className="page-actions">
          <button className="btn btn-primary" disabled={busy || !dirty} onClick={() => void save()}>
            {busy ? t(lang, "stSaving") : t(lang, "stSave")}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── delete confirm ────────────────────────────────────────────────

function DeleteDialog({
  lang,
  card,
  busy,
  onConfirm,
  onClose,
}: {
  lang: Lang;
  card: ToolCard;
  busy: boolean;
  onConfirm: (card: ToolCard, force: boolean) => Promise<void>;
  onClose: () => void;
}): JSX.Element {
  const refs = card.referenced_by;
  return (
    <div className="overlay open" role="presentation" onClick={onClose}>
      <div
        className="dialog st-dialog"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="page-head">
          <h2>{t(lang, "stDeleteTitle")}</h2>
          <button className="btn btn-secondary" onClick={onClose}>
            {t(lang, "stCancel")}
          </button>
        </div>
        <p>{t(lang, "stDeleteAsk").replace("{name}", card.name)}</p>
        {refs.length > 0 && (
          <div className="status error">
            {t(lang, "stDeleteRefs").replace("{names}", refs.join("、"))}
          </div>
        )}
        <div className="page-actions">
          <button
            className={`btn ${refs.length > 0 ? "btn-danger" : "btn-primary"}`}
            disabled={busy}
            onClick={() => void onConfirm(card, refs.length > 0)}
          >
            {t(lang, refs.length > 0 ? "stForceDelete" : "stDelete")}
          </button>
        </div>
      </div>
    </div>
  );
}
