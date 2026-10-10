# 供应商库管理入口修复 + 沙箱模型配置迁移至工作区

## Goal

cc-switch 重构（10-04-model-config-cc-switch）落地后的两处用户反馈修正：

1. mgr 供应商库页在空库时**没有任何新建供应商入口**（鸡生蛋问题），且残留已删除功能的文案；
2. 沙箱级模型配置 UI 挂在沙箱编辑页（创建/编辑流程）里，应改为**像 code-server/终端那样在工作区以 pane 打开**——沙箱创建/编辑流程不再出现模型配置。

## Background

- 10-04 任务把模型配置重构为 cc-switch 模式：mgr 层供应商库（模板源）+ 每沙箱独立 agent 配置；D5 决定不迁移旧数据（库从空开始），D6 删除了"从 pi 导入"。
- 「新增供应商」按钮是 ProviderGrid 尾部的幽灵卡片，而 ProviderGrid 只在库非空时渲染 → 空库永远看不到入口。
- 每沙箱模型配置目前是 mgr-web 沙箱编辑页（EditPage）的原生 Tab，经 mgr `/api/sbx/:name` 代理读写沙箱 app 的 `/api/models/*`。
- 配置文件本身已落在沙箱工作区卷（`workspace:/root`：`/root/.aio/models/config.json` + `~/.claude/settings.json` 等），无需改动存储。

## Requirements

### R1: 供应商库 = 纯供应商管理

- 空库时也必须能新建供应商（入口常驻可见）。
- 移除一切导入类残留文案（"从 pi 导入"字样）；页面只保留：新增、编辑、删除、连通性测试 / 模型拉取（编辑器内已有能力）。
- mgr 顶层「模型供应商」页保留，角色仍是共享模板源，其余不动。

### R2: 沙箱模型配置 UI 迁至工作区 pane

- 移除 mgr-web 沙箱编辑页的「模型配置」Tab；创建/编辑沙箱流程不再出现模型配置 UI。
- 工作区（GoldenLayout）中，每个**运行中**的沙箱可打开「模型配置」pane，交互与 code-server/终端一致（沙箱树按钮 → 打开 pane，标题 `模型配置@<name>`）。
- pane 内容复用现有 `SandboxModelConfigTab`（cc-switch 四 agent 配置 + 模板库下拉），行为不变：仍经 mgr `/api/sbx/:name` 代理读写。
- 后端 API（mgr 代理 + app `/api/models/*`）零改动；Usage 页零改动。

## Constraints

- 纯 mgr-web 前端改动（+ 文档注释），不重建 app/mgr 镜像即可全量生效（对旧镜像沙箱同样生效）。
- 不改动配置存储位置与数据结构（`SandboxAgentConfig` 不变）。
- mgr-web 构建 `tsc --noEmit` 是硬门槛。

## Acceptance Criteria

- [x] AC1 空供应商库：ModelsPage 显示「新增供应商」卡片，点击可新建（含 id/name/baseUrl 表单弹窗）。
- [x] AC2 全站（zh/en 文案）无 "从 pi 导入"/"import from pi" 残留。
- [x] AC3 沙箱编辑页不再有「模型配置」Tab；编辑页剩余内容（env 等）正常。
- [x] AC4 工作区：运行中沙箱的树节点可打开「模型配置」pane，标题 `模型配置@<name>`；多沙箱可同时各开一个 pane。
- [x] AC5 pane 内四 agent（claude/codex/pi/opencode）配置读写、切换、模板库下拉均工作（与原 Tab 行为一致）。
- [x] AC6 `tsc --noEmit && vite build` 通过；spec 中 "page 类型已退役" 的过时指导同步更新。
