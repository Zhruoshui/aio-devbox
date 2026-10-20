# Design — mgr-web 模型配置体验优化六项

前置：代码事实与实测证据见 `research/findings.md`。改动分四个工作面：A) discover 后端修复（R4）、B) 模板 apiKey 透传（R2）、C) 沙箱面板整文件编辑（R1/R6）、D) 布局与多选 UX（R3/R5）。

## D0 总原则

- **单一事实源不变**：沙箱模型配置的事实源仍是 app 侧结构化 config（GET/PUT `/api/models/config`）。「手写原文件」= 前端把文件文本 parse-back 进结构化草稿，再走既有 save → apply 渲染管线。渲染器（app/src/routes/models/render/*）零改动。
- **app/mgr discover 同源**：两处代码按同一逻辑同步修改，纯函数单测同步扩。

## A. discover 2xx-无模型继续候选（R4）

`app/src/routes/models/discover.rs` 与 `mgr/src/models.rs` discover() 主循环：

```
2xx:
  models = parse(body)
  if !models.is_empty() → 返回成功（现状不变）
  else → 记录 no_models_err = "no models parsed from {url} …"；continue   // 唯一改动
```

循环耗尽后的错误返回优先级：`first_auth_failure` > `no_models_err` > `all candidates failed`（新增 `no_models: Option<(String)>` 局部变量，与 `first_auth_failure` 并列）。

理由：bigmodel 首候选 `/api/anthropic/models` 回 200+错误 JSON，次候选 `/api/anthropic/v1/models` 才是真端点；「2xx 即认定端点正确」的假设不成立。401/403 首候选短路的既有逻辑保留。

单测（两侧同步，纯函数层不动，handler 行为层补集成测试或调整既有测试语义）：
- `discover_2xx_no_models_falls_through_to_next_candidate`（mock 顺序：2xx 空 → 2xx 有模型）。

## B. 模板 apiKey 透传（R2）

**后端** `mgr/src/models.rs get_templates()`：json! 增 `"apiKey": p.api_key`（serde Option<String> → null 省略或带 null 均可，用 `if let`/`serde_json::skip` 风格与 providers 路由一致——直接放 json! 宏，null 即前端 `undefined`）。更新注释与既有单测 `get_templates_omits_api_key_entirely` → 改为断言携带真实 key；新增脱敏回归说明：providers 路由的 mask 语义不变。

**前端**：
- `types.ts ProviderTemplatePublic` 加 `apiKey?: string`；`api.ts getProviderTemplates` 解码带出。
- `TemplatePickerModal` 注释更新（移除「NEVER transmitted」表述）；选择回调透传 apiKey。
- `SwitchAgentPanel.onTemplatePicked` / `claudeDefaultsFromTemplate` / `codexTomlFromTemplate`（签名加 `apiKey?: string`）：
  - claude 新建：`env.ANTHROPIC_AUTH_TOKEN = tp.apiKey`（仅当有值）；编辑预填：`if (!env.ANTHROPIC_AUTH_TOKEN && tp.apiKey)` 填入。
  - codex 新建/预填：`auth.OPENAI_API_KEY = tp.apiKey`（同「仅空时填」策略）。
- `IncrementalAgentPanel.onTemplatePicked`：`apiKey: draft.node.apiKey || tp.apiKey`。

**安全记录**：10-04「模板不传 key」决定被本任务显式推翻。信任模型：mgr-web 是唯一管理面，`/api/models/templates` 与 `/api/models/providers`（编辑器可改 key、discover ById 用真实 key）同级信任；不做按沙箱区分。

## C. 四 agent 整文件编辑（R1/R6）

### C1 文件集与 UI 结构（对齐 cc-switch）

| agent | 源码编辑器内容 | 语言 |
| --- | --- | --- |
| claude | `settings.json` 单文件（现状保持） | json |
| codex | `config.toml` + `auth.json` **两段式**（同一编辑器内两个小节，各自校验/保存） | toml + json |
| pi | `models.json` + `settings.json` 两段式 | json |
| opencode | `opencode.jsonc` 单文件 | **jsonc（新增语言枚举）** |

实现载体：`SourceCodeEditor` 复用（已支持 view/edit、JSON 校验）；新增：
- `SourceLanguage += "jsonc"`；`validateSource` 对 jsonc 先剥注释再 JSON.parse。剥注释器放 `sandboxTypes.ts`（或新 `sourceFiles.ts`）：逐字符状态机剔除 `//…\n` 与 `/*…*/`（不处理字符串字面量内注释的边界情况——**要处理**：字符串内 `//` 不算注释，状态机含 in-string/in-escape 态；cc-switch jsonc_document.rs 同款语义）。
- pi 的 `mergedPreview` 拆为两个真实文件的渲染：`modelsFileText(config)`（= `{providers: …}`，与 render/pi.rs 落盘的受管子树一致）与 `settingsFileText(config)`（= `{defaultProvider?, defaultModel?}`）。**移除现有把 defaults 拼进 models.json 的假预览**。
- opencode 单文件沿用 `mergedPreview` 的形状（`{model: "prov/model", provider: {...}}`），即渲染器落盘形状。

### C2 parse-back（文件文本 → 结构化草稿）

新模块 `mgr-web/src/pages/models/sourceFiles.ts`：

```
parseIncrementalFiles(agent, modelsOrJsoncText, settingsText?) →
  { providers: Record<string, ProviderNode>, defaultProvider?, defaultModel? } | { error }
```

- pi：models 文本 → `providers`（逐节点按 `asProviderNode` 语义校验：baseUrl/api 必填字符串）；settings 文本 → defaultProvider/defaultModel。settings 校验失败仅禁用保存并提示（models 合法也不许保存，两文件一体保存）。
- opencode：剥注释 → JSON.parse → 取 `provider`（每个节点 `options.baseURL` → baseUrl、`options.apiKey` → apiKey、`npm` → compat.npm 回填 compat）与顶层 `model`（`"prov/model"` 拆 defaultProvider/defaultModel）。
- codex：`sourceToSnapshotSettings` 扩展——config 段文本直通（现状），auth 段 JSON.parse 得 `auth` 对象（校验为 object）。
- 节点缺 `baseUrl/api` 等 schema 必填：报「节点 {id} 缺少 baseUrl」类错误，指向行内错误条（SourceCodeEditor 的 err 区）。

**受管子树边界提示**（pi/opencode）：编辑器头部 muted 文案「仅覆盖 providers/默认指针；文件中其他键由 apply 原样保留」。

### C3 面板接线

- `IncrementalAgentPanel`：底部整文件区改为两段（pi）或一段（opencode）`SourceCodeEditor`（editMode 可切换）；保存 → parse-back → `onChange(新 IncrementalAgentConfig)`。编辑期间与结构化编辑互斥（进入源码编辑即折叠节点编辑器，反之亦然，避免双入口写冲突——沿用 SwitchAgentPanel 的互斥模式）。
- `SwitchAgentPanel`：codex 源码视图由单 toml 改两段（config.toml textarea 段 + auth.json JSON 段）；`snapshotToSource`/`sourceToSnapshotSettings` 签名扩展或旁路（codex 分支直接组装两段文本）。apply 目标标注改为 `~/.codex/config.toml + ~/.codex/auth.json`。

## D. 布局与多选（R3/R5）

### D1 行式 provider 列表（R3）

`components.css`：新增 `.sbx-list`（垂直列表，每项一行卡片）替代 `.sbx-cards` 网格于两面板；行内结构：

```
.sb-list-row
  ├ .row-main（flex column, min-width:0）
  │   ├ 名称 + current badge + id（mono, ellipsis）
  │   ├ URL 行（mono, ellipsis, title=全 URL）
  │   └ 协议 badge · 模型数（txs muted）
  └ .row-actions（右侧，不换行）
```

关键 CSS：`.row-main { min-width: 0 }` + `ellipsis`；URL 行 `white-space: nowrap; max-width: 100%; overflow: hidden; text-overflow: ellipsis`。Switch 行显示 id/名称（URL 在编辑器里，行上显示 `current` 与名称即可，但为一致性也显示快照 env 里的 BASE_URL 摘要——取 `settingsConfig.env.ANTHROPIC_BASE_URL` 或 codex config 的 base_url 行，取不到显示 `—`）。

### D2 discover 多选增强（R5）

- `ProviderEditorModal` discover 对话框：加「全选 / 清空」按钮对 + 「n / total」计数；列表保持既有 filter/checkbox。
- `IncrementalAgentPanel` 节点编辑器加「获取模型」按钮（baseUrl 非空才可用）→ 调 `discoverModels({baseUrl, api, apiKey})`（mgr literal 分支）→ 复用同一 DiscoverDialog 组件。为此把 discover 对话框从 ProviderEditorModal 抽成共享组件 `components/DiscoverModelsDialog.tsx`（props: state + onSet + onConfirm(ids)）。

## 兼容与回滚

- 全部前端改动，加两处 Rust handler 小改；无 schema/存储迁移。回滚 = revert 提交。
- dist 需重建（mgr 容器直接服务 mgr-web/dist）。
- 风险点：jsonc 剥注释器边界（字符串内 `//`）——单测覆盖；codex auth 段 JSON 校验失败禁保存（与既有 SourceCodeEditor 行为一致）。

## 测试策略

- Rust：app/mgr discover handler 层新增「2xx 空继续候选」测试；get_templates 携 key 断言更新。
- TS：无测试框架（mgr-web 无 vitest 配置——按现状走 build + 手动验证清单，见 implement.md 验证段）。
- 手动验证：bigmodel anthropic 实测拉列表（key 已在任务内提供）；pi/opencode 源码编辑 round-trip；模板选 bigmodel 自动填 key。
