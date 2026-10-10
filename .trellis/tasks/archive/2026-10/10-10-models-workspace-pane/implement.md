# Implement — 执行清单

前置：工作区干净（git status clean），分支策略沿用 main 直提（改动均为 mgr-web 前端 + 文档，无部署脚本变更）。

## Phase A: R1 供应商库入口（独立可验证）

- [x] A1 `ModelsPage.tsx`：删 `providers.length === 0` 空态分支，恒渲染 `ProviderGrid`。
- [x] A2 `i18n.ts`：删 `mcNoProviders`（zh ~369 / en ~918）；`grep -rn "mcNoProviders\|pi 导入\|import from pi" mgr-web/src` 归零。
- [x] A3 验证：`cd mgr-web && npx tsc --noEmit`（未构建全量前先过类型）；浏览器/截图确认空库出「新增供应商」卡片、可新建一条。

## Phase B: R2 模型配置迁工作区 pane

- [x] B1 `SandboxTree.tsx`：放行 `type === "page"`（:136 过滤改掉 + :125 注释更新）。
- [x] B2 `WorkspacePage.tsx`：manifest 拉取后注入合成条目 `{id:"modelsConfig", type:"page", enabled:true, label:t(lang,"edTabModels")}`，带 id 去重。
- [x] B3 新建 `pages/workspace/panes/ModelsPane.tsx`：薄包装（pane 内滚动/最小宽），内部挂 `SandboxModelConfigTab`（sandboxName/sandboxStatus/lang）；`PaneForService` 的 page 分支接到它。
- [x] B4 `EditPage.tsx`：删模型配置 Tab（按钮/分支/import）；单 Tab 剩 env → 移除 Tab 栏。
- [x] B5 CSS：`ModelsPane` 窄容器适配（不新造 token，复用现有布局类）。
- [x] B6 验证（AC3–AC5）：
  - `npx tsc --noEmit && npx vite build` 通过；
  - 工作区树：运行中沙箱出现「模型配置」按钮，开 pane 标题 `模型配置@<name>`；
  - pane 内切四个 agent、编辑保存、模板库下拉填充正常；
  - EditPage 无模型配置 Tab、env 流程完好；
  - 停机沙箱树里无该按钮；
  - 刷新后布局恢复（page componentState 持久化路径）。

## Phase C: 收尾

- [x] C1 `app/services.toml:125` 注释更新（page 渲染复活于 mgr-web、条目客户端合成、勿在此烘焙）。
- [x] C2 spec 更新（Phase 3.3）：`.trellis/spec/frontend/index.md` Pane types 节改写；如 directory-structure.md 提及 Models Tab 位置则同步。
- [x] C3 全量回归：R1 + R2 一起过一遍 AC1–AC6。
- [x] C4 提交（分两个 commit：`fix(mgr-web)` 供应商库空态入口；`refactor(mgr-web)` 模型配置迁工作区 pane + 注释/spec）。

## 验证命令

```bash
cd mgr-web && npx tsc --noEmit && npx vite build   # 构建门槛
grep -rn "mcNoProviders\|pi 导入\|import from pi" src/ || echo clean
```

浏览器验证走既有链路（mgr.localhost；必要时 vnc 容器 puppeteer 截图，参照 frontend-verify-live 工作法）。

## 回滚点

- Phase A、B 各自独立成 commit，可单独 revert。
- 静态产物重发即回滚，无数据迁移。
