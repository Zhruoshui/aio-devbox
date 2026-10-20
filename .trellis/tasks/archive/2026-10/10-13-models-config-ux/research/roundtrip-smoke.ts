import {
  piModelsFileText, piSettingsFileText, opencodeFileText,
  parseIncrementalFiles, codexSourceParts, parseCodexSourceParts,
} from "/home/ruoshui/Documents/MyProject/agent/sandboxes/aio/mgr-web/src/pages/models/sourceFiles";
import { stripJsoncComments } from "/home/ruoshui/Documents/MyProject/agent/sandboxes/aio/mgr-web/src/components/SourceCodeEditor";
import type { IncrementalAgentConfig } from "/home/ruoshui/Documents/MyProject/agent/sandboxes/aio/mgr-web/src/pages/models/sandboxTypes";

let failures = 0;
function check(name: string, cond: boolean, extra?: unknown) {
  if (!cond) { failures++; console.error("FAIL", name, extra ?? ""); }
  else console.log("ok  ", name);
}

// ── jsonc stripper
check("strip keeps url in string", stripJsoncComments(`{"u":"https://x.com/a//b"}`) === `{"u":"https://x.com/a//b"}`);
check("strip line comment", JSON.parse(stripJsoncComments(`{"a":1} // hi`)) .a === 1);
check("strip block comment", JSON.parse(stripJsoncComments(`{/* c */"a":1}`)).a === 1);
check("strip escaped quote in string", stripJsoncComments(`{"a":"x \\" y // z"}`) === `{"a":"x \\" y // z"}`);

// ── pi round-trip
const pi: IncrementalAgentConfig = {
  providers: {
    "bigmodel": {
      name: "智谱", baseUrl: "https://open.bigmodel.cn/api/paas/v4",
      api: "openai-completions", apiKey: "sk-1", headers: {},
      models: [
        { id: "glm-4.6", name: "GLM-4.6", contextWindow: 200000, cost: { input: 0.6, output: 2.2 } },
      ],
    },
  },
  defaultProvider: "bigmodel", defaultModel: "glm-4.6",
};
const piBack = parseIncrementalFiles("pi", piModelsFileText(pi), piSettingsFileText(pi));
check("pi round-trip", JSON.stringify(piBack.config) === JSON.stringify(pi), piBack.error);
const piEdit = parseIncrementalFiles("pi",
  JSON.stringify({ providers: { x: { baseUrl: "https://x", api: "openai-completions" } } }),
  JSON.stringify({ defaultProvider: "x", defaultModel: "m" }));
check("pi edit parse", piEdit.config?.providers.x.baseUrl === "https://x" && piEdit.config?.defaultModel === "m", piEdit.error);
const piBad = parseIncrementalFiles("pi", JSON.stringify({ providers: { x: { api: "openai-completions" } } }), "{}");
check("pi missing baseUrl errors", piBad.error !== undefined && piBad.error.includes("baseUrl"), piBad.error);
const piBadSettings = parseIncrementalFiles("pi", `{"providers":{}}`, `{bad json`);
check("pi bad settings errors", piBadSettings.error !== undefined, piBadSettings.error);

// ── opencode round-trip (with comments!)
const oc: IncrementalAgentConfig = {
  providers: {
    "bigmodel": {
      name: "Zhipu", baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4",
      api: "openai-completions", apiKey: "k9", headers: {},
      compat: { npm: "@ai-sdk/openai-compatible", options: { temperature: 0.2 } },
      models: [{ id: "glm-4.6" }],
    },
  },
  defaultProvider: "bigmodel", defaultModel: "glm-4.6",
};
const ocText = opencodeFileText(oc) // inject a comment line for jsonc realism
  .replace(/^{$/m, `{\n// managed by aio models config`);
const ocBack = parseIncrementalFiles("opencode", ocText);
check("opencode round-trip w/ comment+compat", JSON.stringify(ocBack.config) === JSON.stringify(oc), ocBack.error);

// ── codex two-part
const parts = { configText: 'model = "gpt-5"\nbase_url = "https://x/v1"\n', authText: '{\n  "OPENAI_API_KEY": "sk-9"\n}' };
const parsed = parseCodexSourceParts(parts.configText, parts.authText);
check("codex parse", parsed?.config === parts.configText && (parsed!.auth as any).OPENAI_API_KEY === "sk-9");
check("codex bad auth null", parseCodexSourceParts("x = 1", "{oops") === null);
check("codex empty config null", parseCodexSourceParts("", "{}") === null);
const rt = codexSourceParts({ id: "c", name: "", settingsConfig: parsed! });
check("codex round-trip", rt.configText === parts.configText && rt.authText === JSON.stringify({ OPENAI_API_KEY: "sk-9" }, null, 2));

process.exit(failures === 0 ? 0 : 1);
