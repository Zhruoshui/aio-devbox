# Web 按钮侧栏分组去重 + 注册死端口提示

## Goal

修复用户实测反馈的两个 UX 问题：

1. 用户自注册的 web 型按钮同时出现在侧栏「Web 工具」和「自定义」两个分组（重复），应只出现在「自定义」。
2. 注册时填了一个当前无服务监听的端口（如实测中的 8080），没有任何提示，注册成功但按钮永远灰色、点击 502，用户困惑。

## Background

- 侧栏分组在 `web/src/Sidebar.tsx`：`web` 组按 `type === "web"` 过滤，`custom` 组按 `deletable` 过滤。用户自注册的 web 按钮两个条件同时命中 → 重复出现。
- 注册表单在 `web/src/RegisterDialog.tsx`：web 态只做 1-65535 且 ≠8088 的格式校验，不探活。
- 后端 manifest 已有 TCP 探活语义（`config.rs`，web 按钮 `enabled` 即探活结果），但注册路径完全没有用到。

## Requirements

### R1 侧栏分组去重（前端）

- 用户自注册的按钮（`deletable === true`）只归入「自定义」组。
- 「Web 工具」组仅保留内置 web 服务（code-server / vnc / piWeb 等非 deletable）。
- 终端组现有语义（`type === "agent" && !deletable`）不变；「自定义」组语义不变。

### R2 注册死端口提示（前端 + 后端）

- 后端新增探活端点：`GET /api/buttons/probe?port=<N>`，返回 `200 {"listening": true|false}`；端口非法（非整数 / 越界 / 8088）返回 400。复用 manifest 探活同一 TCP-dial 语义，超时上限与现有一致。
- 表单 web 态下，端口格式合法即防抖探活（约 500ms）：
  - 无服务监听 → 端口字段下方显示**非阻断**警告文案（如「端口 8080 当前无服务监听，注册后按钮将不可用，预览返回 502」）。
  - 有服务监听 → 显示「运行中」正向提示。
  - 格式校验错误（含 8088）时探活提示让位于错误提示。
- **注册行为不变**：探活失败不阻断提交，仍可成功注册（用户可能先注册后起服务）。
- 提示文案中英文齐全（i18n）。

## Acceptance Criteria

- [ ] AC1 自注册 web 按钮只出现在「自定义」组；内置 web 服务仍归「Web 工具」组。
- [ ] AC2 注册表单 web 态输入死端口出现警告提示，输入活端口出现正向提示，格式错误时只显示错误提示。
- [ ] AC3 探活失败仍可提交注册并返回 201。
- [ ] AC4 `GET /api/buttons/probe?port=N`：活端口 `{"listening":true}`、死端口 `{"listening":false}`、非法端口 400；带单测。
- [ ] AC5 `cargo test` 全绿、`cargo clippy` 零警告、`cd web && npm run build` 零错误。
- [ ] AC6 部署到本地 compose 栈后经 UI/API 手工回归：分组正确、提示出现、注册/预览/删除链路无回归。

## Non-goals

- 不阻止任何端口注册（不新增 400 场景，8080 不进黑名单——它只是容器内恰好无服务）。
- 不做已注册按钮的持续健康度告警（manifest 的 `enabled` 字段已承担此职责）。
- 不动 gateway/Caddyfile。

## Notes

- 任务分类：轻量偏中（前端两处 + 后端一个小端点 + i18n + 测试）。PRD + implement.md，无需 design.md（无新边界/契约争议；probe 端点复用既有探活语义）。
- 起因对话：用户实测 issue #1 功能时发现（沙箱 aio-andes，2026-09-02）。
