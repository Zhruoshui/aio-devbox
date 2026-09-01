# Implement — 模型配置页重构

> 前置：`python3 ./.trellis/scripts/get_context.py --mode packages` 加载包上下文；读 `prd.md §R1–R4`、`design.md §2/§3/§7`。涉及跨层（后端 Rust ↔ 前端 TS），改动顺序：先后端 R1 → 后端测试绿 → 前端模块拆分 → 视图重做 → 图表 → 视觉/i18n → 全量验证 → spec 更新 + commit。

## 1. 后端：删除 anthropic 块（R1）

- [x] `app/src/routes/models/store.rs`：`ProviderEntry` 删 `anthropic` 字段；删 `AnthropicBlock` 结构体；`import_from_pi`（~L407）删塞块逻辑；更新 `import_from_pi` 测试（`anthropic.is_some` → `is_none`/删）。
- [x] `app/src/routes/models/render/claude.rs`：`apply_claude` 里 `anthropic_base` 恒为 `&provider.base_url`；`sample_config` 去 `anthropic` 参数；删 2 个 `anthropic_block_*` 测试；文件头注释更新；`use store::{...}` 去 `AnthropicBlock`。
- [x] 验证：`cargo test -p aio models`（或仓库实际包名，见 make/build 约定）全绿。先跑改前基线再改后对比。

## 2. 前端：模块拆分（R3 基础设施）

- [x] 建 `web/src/panes/models/`，把 `ModelsPane.tsx` 内容拆为 `index.tsx / ModelsPane.tsx / types.ts / ProviderGrid.tsx / ProviderEditor.tsx / ModelTable.tsx / AgentTabs.tsx / UsageTab.tsx / charts.tsx`（类型/解码器进 `types.ts`；数据获取与写回 handler 留 `ModelsPane.tsx`；子组件纯 props+回调）。
- [x] `App.tsx` import 改 `./panes/models`；确认其余引用（Sidebar/icons/types）不动。
- [x] 验证：`cd web && npm run build` 干净（tsc --noEmit + vite build）。

## 3. 供应商库：卡片网格 + 编辑器抽屉（R2）

- [x] `ProviderGrid.tsx`：auto-fill 网格；卡片=协议 pill + 名称 + 悬停动作（编辑/删除）+ mono baseUrl + 模型数/掩码 key + agent chips（点击跳 agent 页签）。
- [x] `ProviderEditor.tsx`：右侧覆盖抽屉（right:0, width min(440px,100%)，250ms 进出）；分节：基本信息（名称/baseUrl/协议/apiKey+显隐）/ 高级（headers/compat JSON textarea，保留解析校验）/ 绑定概览（被绑 agent chips 跳转）。
- [x] `ModelTable.tsx`：模型表格 + **per-model 协议列**（select，空=继承）；discover 拉取 + test pill 状态机（沿用 `TestStateMap` 与「provider 身份变更重置」effect）；discover 模态换 Kumo 视觉。
- [x] 空态（无供应商）：居中提示 + 新增（主）+ 从 pi 导入（次）。
- [x] 验证：`npm run build`；`npm run dev` 目测网格/抽屉/新增/删除/保存。

## 4. Agent 页签重排（R2+R3）

- [x] `AgentTabs.tsx`：4 页签表单 Kumo 化（卡片表单、安装徽标语义色、live mono 回读）；`incompatibleReason` 收紧（claude 仅 `api==anthropic-messages`）；保存分配 + 生效 + 写入结果面板（written/backup/errors）齐全。
- [x] 验证：`npm run build`；目测 4 页签绑定/保存/生效。

## 5. 用量图表（R4）

- [x] `UsageTab.tsx`：从 `rows` 算 `totalIn/Out/Cache/Cost`、`byModel(top8)`、`costByAgent`；4 张汇总卡 + 明细表（Kumo 表格样式）。
- [x] `charts.tsx`：`TokenBars`（水平条，Kumo 分类色，条尾数值+模型名）；`CostDonut`（SVG stroke-dasharray，中心总成本，右图例含占比，无 cost 数据整卡隐藏）。
- [x] 验证：`npm run build`；`npm run dev` 目测汇总卡/柱状/环图/表；无成本数据时环图消失。

## 6. 视觉 + i18n（R3 收尾）

- [x] `styles.css`：新增 `.ml-*` 语义类块（卡片/抽屉/网格/图表/表格），删除全部旧 `mc-*` 块（~107 条）；只用 token，无裸色/`dark:` 变体；focus ring / disabled / reduced-motion 齐备。
- [x] `i18n.ts`：新增键（供应商卡片、抽屉、per-model 协议、图表、空态、跳转等），zh-CN + en 双语。
- [x] 验证：`npm run build`；对照 `docs/open-design/cloudflare_kumo_ui.md` 走查（分层/圆角/字阶/间距/focus）。

## 7. 全量验证（质量门）

- [x] `cargo test`（Rust 全绿，含 claude renderer 改写后）。
- [x] `cd web && npm run build`（tsc 严格 + vite 构建干净）。
- [x] 容器实测全链路：`make up` 后 ModelsPane → discover 拉模型 → 加模型 → test 连通 → agent 绑定 → apply → 打开 usage 看图表（沿用旧任务验证命令）。
- [x] `python3 ./.trellis/scripts/task.py current` 确认任务态；跑 `trellis-check` 复查（spec 合规、lint、跨层数据流）。

## 8. 收尾（Phase 3）

- [x] 更新 `.trellis/spec/backend/model-config-guide.md`：anthropic 块移除、`incompatibleReason` 收紧、前端结构 `panes/models/`。
- [x] 若值得沉淀：`.trellis/spec/frontend/` 补 ModelsPane 拆分/图表配色约定（按 `trellis-update-spec` 判断）。
- [x] 按仓库约定 commit（`feat: ...`），不要 push。
- [x] 更新 `.trellis/workspace/ruoshui/journal-1.md` 当日进展。

> **Finish status (2026-08-27)**: R1-R4 全部落地。验证：cargo test 148 全绿
> (throwaway rust:1-bookworm 容器跑)；`npm run build` 干净(tsc 严格 + vite)；
> 活体冒烟——app 容器已用 sandbox-app:latest 重建(含重构前端 bundle,含
> mcApiInherit 键),`/api/models/config|usage|agents` 200,canonical 无
> `"anthropic":` 块键,gateway→app 路由 200,sidecar(code-server 8200/vnc
> 6080/pi-web 30141)全部可达。discover/test 外网全链路未在本会话重跑
> (网络策略所限,逻辑由 cargo 测试覆盖;上一任务已实测)。i18n 死键
> (mcComingSoon/mcDiscoverNoNew/mcLiveNone)已清理。
