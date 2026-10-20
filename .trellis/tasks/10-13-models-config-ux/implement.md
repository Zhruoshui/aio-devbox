# Implement — mgr-web 模型配置体验优化六项

顺序按依赖排：先后端两件（A/B，互不依赖可并行），再前端共享件（B 前端依赖 B 后端；C2 依赖 C1），最后布局与装配。每步含验证命令；全部通过后走 2.2 全量检查。

## Step 1 — A: discover 2xx-无模型继续候选（R4）

- [ ] `mgr/src/models.rs` discover()：`2xx && models.is_empty()` → 记 `no_models: Option<String>` 并 `continue`；耗尽后错误优先级 auth > no_models > tried。
- [ ] `app/src/routes/models/discover.rs`：同款修改（保持两文件同源风格）。
- [ ] 双侧补 handler 级测试：首候选 2xx 空 body/错误 JSON、次候选正常 → 成功返回。
- [ ] 验证：`cargo test -p mgr -p app models`；手动 curl 走 mgr `/api/models/discover`（bigmodel anthropic literal）见 GLM 列表。

## Step 2 — B后端: get_templates 携带 apiKey（R2）

- [ ] `mgr/src/models.rs get_templates()` json! 加 `"apiKey": p.api_key`；更新 doc 注释（记录推翻 10-04 决定 + 信任模型）。
- [ ] 更新单测 `get_templates_omits_api_key_entirely` → 携真实 key 断言。
- [ ] 验证：`cargo test -p mgr models`。

## Step 3 — B前端: 模板选择自动填 key（R2，依赖 Step 2）

- [ ] `types.ts ProviderTemplatePublic.apiKey?: string` + `api.ts` 解码。
- [ ] `TemplatePickerModal` 注释更新，onSelect 透传 apiKey。
- [ ] `SwitchAgentPanel`：`claudeDefaultsFromTemplate`/`codexTomlFromTemplate`/`onTemplatePicked` 填 AUTH_TOKEN / auth.OPENAI_API_KEY（仅空时填）。
- [ ] `IncrementalAgentPanel.onTemplatePicked` 填 node.apiKey（仅空时填）。
- [ ] 验证：`npm run typecheck`；手动：选 bigmodel 模板后四个面板 key 均出现。

## Step 4 — C1: SourceCodeEditor 扩 jsonc + sourceFiles 模块（R6 基座）

- [ ] `SourceCodeEditor`：`SourceLanguage += "jsonc"`；`validateSource` jsonc = 剥注释 + JSON.parse。
- [ ] 新 `mgr-web/src/pages/models/sourceFiles.ts`：
  - `stripJsoncComments(text)`（状态机：normal/in-string/in-escape/in-line-comment/in-block-comment，字符串内 `//` 不剥）。
  - `modelsFileText(pi config)` / `settingsFileText(pi config)`（真实落盘形状，替代 mergedPreview 的 pi 分支）。
  - `opencodeFileText(config)`（= 现 mergedPreview opencode 分支独立成函数）。
  - `parseIncrementalFiles(agent, mainText, settingsText?)` → 结构化或 `{error}`（节点校验：baseUrl/api 必填）。
  - codex 两段：`codexSourceParts(snapshot)` / `parseCodexSourceParts(configText, authText, prev)`。
- [ ] 验证：`npm run typecheck`。

## Step 5 — C3: 面板接线整文件编辑（R1/R6，依赖 Step 4）

- [ ] `IncrementalAgentPanel`：底部只读 `<pre>` 替换为 SourceCodeEditor（pi 两段 / opencode 一段 jsonc），编辑态与节点编辑器互斥；保存 = parse-back → onChange。头部加「受管子树」提示文案（i18n zh/en）。
- [ ] `SwitchAgentPanel`：codex 源码视图改两段（config.toml + auth.json）；`sandboxTypes.snapshotToSource/sourceToSnapshotSettings` codex 分支改造或旁路；apply 目标标注 `~/.codex/config.toml + ~/.codex/auth.json`。
- [ ] 删除 `mergedPreview`（被 sourceFiles 函数取代）。
- [ ] i18n：新增 key（受管子集提示、auth.json 段标题、settings.json 段标题等）zh/en 双语。
- [ ] 验证：`npm run typecheck`；手动 round-trip：编辑→保存→再打开内容一致；pi defaults 改动落 settings 段。

## Step 6 — D1: 行式 provider 列表（R3）

- [ ] `components.css` 新增 `.sbx-list`/`.sbx-row` 行式样式（min-width:0 + ellipsis + title）。
- [ ] `SwitchAgentPanel` / `IncrementalAgentPanel` 卡片区换行式列表（Switch 行加 BASE_URL 摘要行）。
- [ ] 验证：`npm run build`；手动：超长 URL（≥120 字符）行不溢出、悬停见全文、按钮可点。

## Step 7 — D2: discover 对话框共享化 + 多选增强（R5）

- [ ] 抽 `components/DiscoverModelsDialog.tsx`（props: discover state, onSet, onConfirm；ProviderEditorModal 改用）。
- [ ] 对话框加「全选/清空」+ `n / total` 计数。
- [ ] `IncrementalAgentPanel` 节点编辑器加「获取模型」按钮（baseUrl 非空可用）→ `discoverModels` literal → 对话框多选 → 并入 node.models（去重）。
- [ ] i18n：全选/清空/获取模型等新 key。
- [ ] 验证：`npm run build`；手动：bigmodel openai 协议 + anthropic 协议各拉一次。

## Step 8 — 收尾

- [ ] `cd mgr-web && npm run build`（刷新 dist）。
- [ ] `cargo test -p mgr -p app`（全量，防跨模块回归）。
- [ ] 手动全链路：工作区 modelsConfig pane → 四 agent 逐一验证 AC1–AC6。
- [ ] 更新 `.trellis/spec/frontend`（若形成新约定：源码编辑器语言扩展/共享对话框模式）。

## 回滚点

每 Step 一个 commit 粒度；任何一步可独立 revert。Step 1/2 是纯后端，回滚不影响前端编译（templates 的 apiKey 为可选字段）。

## Review gates

- Step 1 后：确认 app/mgr 两处 diff 对称。
- Step 5 后：UI 走查（pi 两段编辑互斥 + round-trip）。
- Step 8 前：2.2 全量质量检查（trellis-check）。
