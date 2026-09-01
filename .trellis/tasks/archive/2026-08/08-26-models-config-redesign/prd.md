# 模型配置页重构——cc-switch 供应商卡片 + Kumo 前端重做 + 用量图表

## Goal

修复统一模型配置页（08-26-unified-model-config 提交引入）的四个问题：
1. 删掉冗余的「Anthropic 协议」复选框——协议选择即可决定端点。
2. 模型配置思路改为 **cc-switch 风格供应商卡片**：供应商=卡片式管理 + 每个供应商内含完整模型库 + per-agent 绑定，不再让用户觉得「一个 agent 只能配一个模型」。
3. 前端视觉按 `docs/open-design/cloudflare_kumo_ui.md` 全部重做（当前 `mc-*` 样式偏离该设计语言）。
4. 用量统计用图表（汇总卡 + 柱状图 + 环图）替代纯表格，走设计文档的图表配色规范。

参考项目：
- pi: https://github.com/agegr/pi-web（模型库数据模型）
- opencode / claudecode / codex: https://github.com/farion1231/cc-switch（供应商卡片 UX）

## Requirements

### R1 删除「Anthropic 协议」复选框（数据模型简化）
- 删除 canonical 配置里 provider 的 `anthropic` 块（`AnthropicBlock`），UI 上的「Anthropic 协议」复选框随之消失。
- 协议（`api`）选 `anthropic-messages` 时，Claude 渲染器直接用 `baseUrl` 作为 `ANTHROPIC_BASE_URL`（与 pi 一致）。
- 旧的 `models.json` 里残留的 `anthropic` 字段：反序列化忽略、下次保存时自然丢弃，不迁移、不报错。

### R2 模型配置思路 → cc-switch 供应商卡片
- 「供应商库」页签改为**卡片网格**：每张卡片显示 名称 / 协议徽标 / Base URL（mono）/ 模型数 / 被哪些 agent 绑定。
- 点卡片打开**编辑器抽屉**（覆盖层），分节组织：基本信息（名称/baseUrl/协议/apiKey/高级 headers/compat）、模型列表（含 per-model 协议覆盖、discover 拉取、连通性检测）、被绑定 agent 概览。
- 模型列表补齐 **per-model 协议列**（后端 `ModelEntry.api` 已存在但 UI 未暴露）。
- 保留 4 个 agent 页签（pi/opencode/claude/codex）做绑定管理：每个 agent 可绑 provider + 主模型 + agent 专属覆盖（claude 三档模型 + authField、codex reasoningEffort + wireApi）。
- 编辑器底部显示「被此供应商绑定的 agent」chips，点击跳转对应 agent 页签。

### R3 前端按 Kumo 设计语言重做
- 全 pane 使用语义 token（`--bg/--surface/--surface-warm/--border/--border-soft/--accent/--muted`），无裸色值、无手动 dark variant。
- 分层：canvas → 卡片（ring + 小阴影, 8px 圆角）→ recessed 区；不嵌套 LayerCard。
- 紧凑排版：Inter、正文 14px、标题 16/20px sentence-case 且 weight 600、4px 节奏、图标与首行文字对齐。
- 交互态齐全：hover / active / focus-visible（2px ring）/ disabled / loading；reduced-motion 尊重 `--motion-fast`。
- 无障碍：字段有可见 label、icon-only 按钮有 aria-label、图表不只靠颜色（配图例/数值）。

### R4 用量统计图表化
- 纯前端，复用现有 `GET /api/models/usage` 聚合数据（不加按天时序，不动后端）。
- 新增：汇总卡（输入/输出/缓存/成本）、按模型的 token 柱状图（水平条）、按 agent 的成本占比环图。
- 图表配色遵循设计文档：分类色 `#4290F0 #F5B647 #E8649D #8D58EE #50C3B6 #D37536`；无成本数据时环图降级隐藏、不占位报错。
- 现有明细表保留并重排为 Kumo 表格。

## Acceptance Criteria

- [ ] **R1**：UI 与 canonical 均无 `anthropic` 字段；`apply claude`（协议=anthropic-messages）写出的 `ANTHROPIC_BASE_URL` == provider `baseUrl`；`anthropic_block_*` 旧测试移除/改写，Rust 测试全绿。
- [ ] **R1**：含旧 `anthropic` 字段的 `models.json` 可正常 GET/PUT（不报错、字段被忽略并随保存丢弃）。
- [ ] **R2**：供应商库页签为卡片网格，点卡片打开编辑器抽屉；模型表格含 per-model「协议」列；discover/test 在编辑器内可用；被绑定 agent chips 可跳转。
- [ ] **R2**：4 个 agent 页签保留 provider+model 绑定与各自覆盖项，保存/生效/写入文件结果面板齐全；未安装 agent 仍可预写配置。
- [ ] **R3**：视觉走查通过——无裸色值/手动 dark 类；卡片 ring+阴影、8px 圆角；正文 14px、标题 sentence-case 600；focus ring、disabled、loading 齐备；`prefers-reduced-motion` 生效。
- [ ] **R3**：`npm run build`（tsc --noEmit + vite build）干净；`web/src/panes/models/` 模块拆分后 `App.tsx` 导入稳定。
- [ ] **R4**：用量页签含 4 张汇总卡 + 水平柱状图 + 成本环图 + 明细表；配色取自 Kumo 分类色；无成本数据时环图隐藏。
- [ ] **回归**：容器实测 `discover → 加入模型 → test → agent 绑定 → apply → usage` 全链路绿（沿用旧任务验证命令）。
- [ ] **spec**：更新 `.trellis/spec/backend/model-config-guide.md`（anthropic 块移除、前端结构变更）。

## Notes

- 范围：只动统一模型配置页；pi-web/cc-switch 本体、其他 pane、页脚、布局持久化等一律不碰。
- 图表不引入新依赖（手写 SVG/div，避免 chart lib 与 React 18 版本耦合）。
- 决策记录见 `design.md §0`；执行清单见 `implement.md`。
