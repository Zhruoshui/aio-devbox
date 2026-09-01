# claude/codex 多配置项(cc-switch 式 preset)

> 父任务 08-27-models-config-v2 的 R4。claude 与 codex 是「切换式」agent
> (一次一套模型生效),按 cc-switch 提供 N 个 preset + 一个 current,页面内一键切换。
> 调研:`research/cc-switch-config-model.md`(§1 ProviderManager、§3 切换机制、§6 取舍)。

## Goal

canonical `agents.{claude,codex}` 从单一 assignment 改为 `{presets[], current}`;
前端 cc-switch 式卡片列表(当前徽标 + 一键切换 + 编辑/复制/删除);渲染器 apply 当前 preset。

## Requirements

### canonical schema 变更(后端,先落)

- `agents.claude: Option<ClaudeAssignment>` -> `Option<ClaudePresets>`,
  其中 `ClaudePresets { presets: Vec<ClaudePreset>, current: Option<String> }`,
  `ClaudePreset { id, name, provider, model, haiku_model?, sonnet_model?, opus_model?, auth_field }`。
- `agents.codex` 同构:`CodexPresets { presets: Vec<CodexPreset>, current: Option<String> }`,
  `CodexPreset { id, name, provider, model, reasoning_effort?, wire_api }`。
- preset `id` 自动生成(短 kebab),`name` 用户起(默认「默认配置」)。
- **向后兼容**:旧 models.json 单 assignment 反序列化 -> 转成 `presets=[单条]`, `current=该条id`;
  保存后落新形状。`anthropic` 块已在 R1 删除,不涉及。

### 渲染器(apply 当前 preset)

- `apply_claude`/`apply_codex`:取 `current` 指向的 preset,其余字段不变地写
  (逻辑与现状等价,仅多了「在 presets 里找 current」一步)。
- current 为 None / 找不到 -> push_err(「无 current preset」),不写半截文件。

### 前端 UI(cc-switch 式)

- claude/codex 页签:卡片列表(名称 / 模型 / 协议徽标 / 「当前」徽标 / 健康可选);
  行操作:**一键切换为当前**(=apply)、编辑、复制、删除。
- 新增 preset:选供应商(走 ModelPicker) -> 选模型 -> 填覆盖项
  (claude 三档模型 + authField;codex reasoningEffort + wireApi)。
- 切换 = `agents.{agent}.current = id` + 保存 + apply;前端即时反映 live 回读。

### GET /api/models/agents live 回读

- claude/codex 的 live 回读读 `~/.claude/settings.json` env / `~/.codex/config.toml`
  的 model+provider,与 current preset 对照,显示「当前生效与 current preset 是否一致」。

## Acceptance Criteria

- [x] schema 迁移:旧单 assignment models.json 可读(转单 preset 默认项)、可保存新形状;新形状可正常读。
- [x] apply 当前 preset:claude 写 ANTHROPIC_* 与之前等价;codex 写 config.toml+auth.json 等价;current 缺失时报错不写半截。
- [x] UI:多 preset 卡片列表、当前徽标、一键切换、复制、删除、新增全流程可用;切换后 live 回读反映。
- [x] 删除当前 preset 时:current 顺移到另一项或置 None(不悬空),apply 拒绝悬空 current 并报错。
- [x] `cargo test` 绿(schema 迁移 + 渲染器各覆盖 preset 多条/单条/空/旧形状);`npm run build` 干净。
- [x] container 实测:新增 2 preset -> 切换 -> apply -> 文件内容随 preset 变。

## Notes

- **派生范式**:preset 引用 providerId + model + 覆盖项,不复制 settingsConfig blob
  (与「供应商库 SSOT」一致,见 cc-switch 调研 §6)。
- 不做 cc-switch 的 backfill 快照(canonical 库是 SSOT,键级合并已保护用户键)。
- preset 名/排序:`Vec` 保序,前端按数组序展示;切换不改数组序,只改 current。
