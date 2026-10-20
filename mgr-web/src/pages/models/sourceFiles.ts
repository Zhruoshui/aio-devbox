// sourceFiles — whole-file source rendering + parse-back for the sandbox
// model-config tab (task 10-13-models-config-ux R1/R6).
//
// "手写原文件" aligned with cc-switch's managed-file set (research F2):
//   pi       ~/.pi/agent/models.json   (managed subtree: providers)
//            ~/.pi/agent/settings.json (defaultProvider / defaultModel)
//   opencode ~/.config/opencode/opencode.jsonc (provider + model keys)
//   codex    ~/.codex/config.toml + ~/.codex/auth.json (two-part helpers)
//   claude   ~/.claude/settings.json (already covered by snapshotToSource)
//
// The structured draft config stays the SINGLE SOURCE OF TRUTH: these
// helpers render the managed subtree of each native file from the config,
// and parse edits BACK into the config (validation errors block the save).
// Keys outside the managed subtree live only on disk — the app renderers
// preserve them at apply time; the editor surfaces that via a hint.

import { stripJsoncComments } from "../../components/SourceCodeEditor";
import type {
  AgentId,
  IncrementalAgentConfig,
  IncrementalModelEntry,
  ProviderNode,
  ProviderSnapshot,
} from "./sandboxTypes";

// ── render: config → file text ─────────────────────────────────────

/** The pi models.json managed subtree: `{ "providers": { … } }` — exactly
 *  the object render/pi.rs replaces inside the on-disk models.json. */
export function piModelsFileText(config: IncrementalAgentConfig | undefined): string {
  const providers: Record<string, unknown> = {};
  for (const [id, node] of Object.entries(config?.providers ?? {})) {
    providers[id] = node;
  }
  return JSON.stringify({ providers }, null, 2);
}

/** The pi settings.json managed keys: defaultProvider / defaultModel (the
 *  rest of that file belongs to pi itself — render/pi.rs only sets these
 *  two). */
export function piSettingsFileText(config: IncrementalAgentConfig | undefined): string {
  const o: Record<string, unknown> = {};
  if (config?.defaultProvider) o.defaultProvider = config.defaultProvider;
  if (config?.defaultModel) o.defaultModel = config.defaultModel;
  return JSON.stringify(o, null, 2);
}

/** The opencode opencode.jsonc managed shape: top `model` pointer plus the
 *  `provider.<id>` nodes (render/opencode.rs's write set). */
export function opencodeFileText(config: IncrementalAgentConfig | undefined): string {
  const provider: Record<string, unknown> = {};
  for (const [id, node] of Object.entries(config?.providers ?? {})) {
    const compat = (node.compat ?? {}) as OpenCompat;
    provider[id] = {
      npm: compat.npm ?? "@ai-sdk/openai-compatible",
      name: node.name || id,
      options: {
        ...(compat.options ?? {}),
        baseURL: compat.options?.baseURL ?? node.baseUrl,
        apiKey: compat.options?.apiKey ?? node.apiKey ?? "",
      },
      models: Object.fromEntries(
        node.models.map((m) => [m.id, { name: m.name ?? m.id }]),
      ),
    };
  }
  const top: Record<string, unknown> = { provider };
  if (config?.defaultProvider) {
    top.model = `${config.defaultProvider}/${config.defaultModel ?? ""}`;
  }
  return JSON.stringify(top, null, 2);
}

/** codex snapshot → the two editable file texts (config.toml + auth.json). */
export function codexSourceParts(snapshot: ProviderSnapshot): {
  configText: string;
  authText: string;
} {
  const cfg = snapshot.settingsConfig.config;
  const configText = typeof cfg === "string" ? cfg : "";
  const auth = snapshot.settingsConfig.auth;
  const authText =
    auth == null || typeof auth !== "object"
      ? "{\n}"
      : JSON.stringify(auth, null, 2);
  return { configText, authText };
}

// ── parse-back: file text → config ──────────────────────────────────

/** opencode node shape wraps the base fields under an `options` blob. */
interface OpenCompat {
  npm?: string;
  name?: string;
  options?: { baseURL?: string; apiKey?: string };
}

function isObj(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null;
}

/** Simple baseUrl shape check (follow-up: 语法层面检查) — non-empty and
 *  http(s). Not URL-parse-complete; catches the hand-slip class. */
export function isValidBaseUrl(u: string): boolean {
  return /^https?:\/\/\S+$/.test(u.trim());
}

/** Parse one pi/opencode provider node; `where` names the file+key for the
 *  error message. baseUrl + api are schema-required (the app's
 *  asProviderNode contract). */
function parseNode(v: unknown, where: string): ProviderNode | string {
  if (!isObj(v)) return `${where}: not an object`;
  if (typeof v.baseUrl !== "string" || v.baseUrl.trim() === "") {
    return `${where}: baseUrl is required`;
  }
  if (!isValidBaseUrl(v.baseUrl)) {
    return `${where}: baseUrl must be an http(s) URL`;
  }
  if (typeof v.api !== "string" || v.api.trim() === "") {
    return `${where}: api is required`;
  }
  const headers: Record<string, string> = {};
  if (isObj(v.headers)) {
    for (const [k, val] of Object.entries(v.headers)) {
      if (typeof val === "string") headers[k] = val;
    }
  }
  const models: IncrementalModelEntry[] = [];
  if (Array.isArray(v.models)) {
    for (const m of v.models) {
      if (!isObj(m) || typeof m.id !== "string" || m.id === "") continue;
      models.push({
        id: m.id,
        name: typeof m.name === "string" ? m.name : undefined,
        reasoning: typeof m.reasoning === "boolean" ? m.reasoning : undefined,
        contextWindow:
          typeof m.contextWindow === "number" ? m.contextWindow : undefined,
        maxTokens: typeof m.maxTokens === "number" ? m.maxTokens : undefined,
        cost: isObj(m.cost)
          ? {
              input: typeof m.cost.input === "number" ? m.cost.input : undefined,
              output: typeof m.cost.output === "number" ? m.cost.output : undefined,
              cacheRead:
                typeof m.cost.cacheRead === "number" ? m.cost.cacheRead : undefined,
              cacheWrite:
                typeof m.cost.cacheWrite === "number" ? m.cost.cacheWrite : undefined,
            }
          : undefined,
      });
    }
  }
  return {
    name: typeof v.name === "string" ? v.name : "",
    baseUrl: v.baseUrl,
    api: v.api,
    apiKey: typeof v.apiKey === "string" && v.apiKey !== "" ? v.apiKey : undefined,
    headers,
    compat: v.compat,
    models,
  };
}

export interface ParseIncrementalResult {
  config?: IncrementalAgentConfig;
  error?: string;
}

/** Parse the edited file text(s) back into an IncrementalAgentConfig.
 *
 *  - pi: `mainText` = models.json managed subtree, `settingsText` =
 *    settings.json managed keys. Both must parse (they save together).
 *  - opencode: `mainText` = the jsonc file (comments allowed); provider
 *    nodes reverse the options.baseURL/apiKey wrapping, the top `model`
 *    pointer becomes defaultProvider/defaultModel.
 *
 *  Returns {config} on success or {error} explaining the first problem. */
export function parseIncrementalFiles(
  agent: AgentId,
  mainText: string,
  settingsText?: string,
): ParseIncrementalResult {
  const stripped = agent === "opencode" ? stripJsoncComments(mainText) : mainText;
  let root: unknown;
  try {
    root = JSON.parse(stripped);
  } catch (e) {
    return {
      error: e instanceof Error ? e.message : String(e),
    };
  }
  if (!isObj(root)) return { error: "root is not a JSON object" };

  const providers: Record<string, ProviderNode> = {};
  let defaultProvider: string | undefined;
  let defaultModel: string | undefined;

  if (agent === "pi") {
    const raw = root.providers;
    if (raw === undefined) {
      // An empty managed subtree — allowed (clears all pi providers).
    } else if (!isObj(raw)) {
      return { error: "providers: not an object" };
    } else {
      for (const [id, v] of Object.entries(raw)) {
        const node = parseNode(v, `providers.${id}`);
        if (typeof node === "string") return { error: node };
        providers[id] = node;
      }
    }

    if (settingsText !== undefined) {
      let settings: unknown;
      try {
        settings = JSON.parse(settingsText);
      } catch (e) {
        return {
          error: `settings.json: ${e instanceof Error ? e.message : String(e)}`,
        };
      }
      if (!isObj(settings)) return { error: "settings.json: not a JSON object" };
      const dp = settings.defaultProvider;
      const dm = settings.defaultModel;
      if (dp !== undefined && typeof dp !== "string") {
        return { error: "settings.json: defaultProvider must be a string" };
      }
      if (dm !== undefined && typeof dm !== "string") {
        return { error: "settings.json: defaultModel must be a string" };
      }
      defaultProvider = dp || undefined;
      defaultModel = dm || undefined;
    }
  } else {
    // opencode
    const raw = root.provider;
    if (raw === undefined) {
      // No provider key — allowed (clears all opencode providers).
    } else if (!isObj(raw)) {
      return { error: "provider: not an object" };
    } else {
      for (const [id, v] of Object.entries(raw)) {
        if (!isObj(v)) return { error: `provider.${id}: not an object` };
        const options = isObj(v.options) ? v.options : {};
        const merged: Record<string, unknown> = {
          ...v,
          baseUrl:
            typeof options.baseURL === "string" ? options.baseURL : undefined,
          apiKey: typeof options.apiKey === "string" ? options.apiKey : undefined,
          // opencode.jsonc carries no protocol field (the npm adapter IS
          // the protocol) — parse-back falls back to the openai default.
          api: typeof v.api === "string" ? v.api : "openai-completions",
        };
        delete merged.options;
        // opencode's models are a MAP (key = id) — normalize to the array
        // form parseNode understands.
        if (isObj(v.models)) {
          merged.models = Object.entries(v.models).map(([mid, mm]) => ({
            id: mid,
            ...(isObj(mm) ? mm : {}),
          }));
        }
        // The opencode-only wrapping (npm + non-baseURL options) rides in
        // compat so the next render puts it back.
        const compat: Record<string, unknown> = {};
        if (typeof v.npm === "string") compat.npm = v.npm;
        if (isObj(v.options)) {
          const rest: Record<string, unknown> = { ...(v.options as Record<string, unknown>) };
          delete rest.baseURL;
          delete rest.apiKey;
          if (Object.keys(rest).length > 0) compat.options = rest;
        }
        if (Object.keys(compat).length > 0) merged.compat = compat;
        const node = parseNode(merged, `provider.${id}`);
        if (typeof node === "string") return { error: node };
        // Canonicalize the always-written defaults back to their omitted
        // forms so a render→parse round-trip is the identity: models render
        // `name` even when absent (defaults to the id) and npm renders the
        // default adapter even when unset.
        node.models = node.models.map((m) =>
          m.name === m.id ? { ...m, name: undefined } : m,
        );
        const c = node.compat as OpenCompat | undefined;
        if (
          c &&
          c.npm === "@ai-sdk/openai-compatible" &&
          Object.keys(c).length === 1
        ) {
          node.compat = undefined;
        }
        providers[id] = node;
      }
    }

    const model = root.model;
    if (model !== undefined) {
      if (typeof model !== "string") return { error: "model: must be a string" };
      const [p, m] = model.split("/");
      defaultProvider = p || undefined;
      defaultModel = m || undefined;
    }
  }

  // Dangling default pointer: the app validates on apply; clear it here so
  // the draft stays self-consistent.
  if (defaultProvider !== undefined && !(defaultProvider in providers)) {
    defaultProvider = undefined;
    defaultModel = undefined;
  }

  return {
    config: {
      providers,
      defaultProvider,
      defaultModel,
    },
  };
}

/** Parse codex's two file texts back into a settings_config object.
 *  config.toml passes through verbatim (validated by codex at runtime);
 *  auth.json must parse as a JSON object. Returns null on bad auth JSON. */
export function parseCodexSourceParts(
  configText: string,
  authText: string,
): Record<string, unknown> | null {
  if (configText.trim() === "") return null;
  try {
    const auth: unknown = JSON.parse(authText.trim() === "" ? "{}" : authText);
    if (!isObj(auth)) return null;
    return { config: configText, auth };
  } catch {
    return null;
  }
}
