# mgr-web 模型配置体验优化六项

## Goal

用户在 mgr-web 模型配置域（工作区 modelsConfig pane + 供应商库页）反馈 6 项改进：文件可编辑、模板自动填 key、卡片防溢出、修 anthropic 拉取、模型多选、四 agent 手写原文件（对齐 cc-switch）。

## Requirements

### R1 沙箱配置「文件预览」可编辑（= pi/opencode 整文件视图）

单个沙箱的模型配置 pane 中，pi/opencode 的整文件视图目前只读（`mergedPreview`）。改为可编辑：用户可自由修改文件内容并保存进草稿配置。

### R2 从模型库选择时自动填 apiKey

沙箱配置面板通过「从模型库选择」预填 provider 时，除 baseUrl/api/models 外，**apiKey 也要自动带入**：
- claude → `ANTHROPIC_AUTH_TOKEN`
- codex → `auth.OPENAI_API_KEY`
- pi / opencode → 节点 `apiKey`

推翻 10-04 PRD「模板永不携带 apiKey」的决定（mgr-web 是唯一管理面，与供应商库编辑器同信任域；设计文档记录该决策变更）。

### R3 模型配置卡片改行式列表，防 URL 溢出

沙箱配置面板的 provider 卡片（`.sbx-cards` 网格）中长 URL 溢出卡片。改为**一行一个配置项的列表式布局**：名称/id、URL、协议、模型数各自成行，URL 超长安全截断（ellipsis + title 提示），操作按钮不被挤压。适用于 SwitchAgentPanel 与 IncrementalAgentPanel 两处卡片列表。

### R4 修复 anthropic 协议拉不到模型列表

供应商库配置 anthropic 协议（如 `https://open.bigmodel.cn/api/anthropic` + 有效 key）后点「获取模型」失败。根因（实测，research/findings.md F1）：首个候选 URL 返回 2xx + 不可解析 body 时，discover 立即报错不再尝试后续候选。修复须同步落在 `app/src/routes/models/discover.rs` 与 `mgr/src/models.rs` 两处同源代码。

### R5 拉取的模型以列表呈现、可多选

模型拉取结果以垂直列表呈现、复选框多选后批量加入（供应商库 discover 对话框已具备，补全选/清空）；并在沙箱增量面板（pi/opencode 节点编辑器）新增「获取模型」入口，走 mgr `/api/models/discover` literal 分支，多选后并入节点 models。

### R6 四类 agent 配置页支持手写原文件（对齐 cc-switch）

claude / codex / pi / opencode 四个配置页均支持手动编辑「原文件」，文件集合与 cc-switch 对齐（research/findings.md F2）：

| agent | 可手写文件 | 现状 → 目标 |
| --- | --- | --- |
| claude | `~/.claude/settings.json` | 已可编辑（保持） |
| codex | `~/.codex/config.toml` + `~/.codex/auth.json` | 源码编辑仅 config.toml → 补 auth.json |
| pi | `~/.pi/agent/models.json` + `~/.pi/agent/settings.json` | 假合并只读预览 → 两文件分别可编辑 |
| opencode | `~/.config/opencode/opencode.jsonc` | 假合并只读预览 → 单文件可编辑（JSONC） |

编辑结果 parse-back 回结构化草稿配置（单一事实源不变，apply 渲染器不改行为）。pi/opencode 编辑器仅覆盖受管子树（providers/defaults），磁盘上其他未知键由渲染器原样保留，需在 UI 提示。

## Constraints

- 不改 aio-models 结构化 schema（IncrementalAgentConfig / SwitchAgentConfig 维持现状，手写原文件通过前端 parse-back 进入现有 schema）。
- app/mgr 两处 discover 代码保持 byte-for-byte 同源风格（含单测同步）。
- 离线环境：不新增 npm 依赖；JSONC 解析手写注释剥离器。
- UI 文案走 i18n（zh/en 双语）。
- 交付含 `npm run build` 刷新 mgr-web/dist。

## Acceptance Criteria

- [ ] AC1 (R1/R6)：pi 面板可分别编辑 models.json 与 settings.json 内容；opencode 面板可编辑 opencode.jsonc（带注释可保存）；保存后草稿配置与预览一致，apply 正常落盘。〔机制已验证：roundtrip-smoke 14/14；UI 走查待部署〕
- [ ] AC2 (R6)：codex 源码编辑覆盖 config.toml 与 auth.json 两个文件；claude 源码编辑不回归。〔代码完成,UI 走查待部署〕
- [ ] AC3 (R2)：从模型库选择后，claude 的 AUTH_TOKEN / codex 的 auth key / pi·opencode 的 apiKey 均自动填入（仅当目标字段为空时填，不覆盖用户已填值）。〔代码完成,UI 走查待部署〕
- [ ] AC4 (R3)：两个面板的 provider 列表改行式布局，超长 URL 不溢出、不撑破卡片，悬停可见全 URL。〔代码完成,UI 走查待部署〕
- [x] AC5 (R4)：bigmodel anthropic 端点点「获取模型」返回 GLM 模型列表；app/mgr 双侧单测覆盖「2xx 无模型 → 继续候选」。〔curl 实测 /api/anthropic/v1/models 返回 GLM 列表；mock handler 测试双侧通过〕
- [ ] AC6 (R5)：discover 对话框支持全选/清空；增量面板节点编辑器可拉取模型并多选加入。〔代码完成,UI 走查待部署〕
- [x] AC7：`cd mgr-web && npm run build` 通过（含 tsc 门禁）；`cargo test -p aio-app -p aio-mgr` 全绿（159+131）。

## Notes

- 研究证据与代码落点：`research/findings.md`。
- R1 与 R6 是同一工作面（pi/opencode 整文件编辑），实现上合并交付。
