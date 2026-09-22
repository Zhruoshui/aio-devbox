# S4 用量图表：分沙箱条形 + 按天趋势

父任务：09-10-mgr-web-ux-batch2（决策 D5，全量背景见父 prd.md）。

## Goal

合计视图补「分沙箱耗费」图；单沙箱视图补「按天趋势」图与时间维度明细。

## Requirements

- R1 mgr/src/usage.rs：合计响应附带每沙箱聚合（token/成本），mgr-web
  渲染横向条形图（点条跳该沙箱视图）——纯拼接，不重算
- R2 app usage.rs：增 byDay 聚合（近 14 天，token 输入/输出/缓存 + 成本
  按日汇总），usage 响应附带；时间处理复用现有 days_from_civil 基础设施
- R3 UsagePage 单沙箱视图：按天趋势图（柱状双系列 token，成本折线叠加，
  自研 SVG，风格对齐 charts.tsx 现有 TokenBars/CostDonut）
- R4 明细表加日期列 + 按日筛选（单沙箱视图）
- R5 opencode 用量解析明确不做（父任务 Out of Scope）

## Acceptance Criteria

> 2026-09-22 实机验收（puppeteer + 容器内 chromium，打运行中的 mgr
> `http://mgr.localhost/`，脚本 `/tmp/mgrverify/final.mjs` + `s4.mjs`）。

- [x] AC1 合计视图显示分沙箱条形图，点条切换到该沙箱
      —— 合计视图标题「各沙箱用量」、激活 chip「全部合计」；点第一条
      `.ml-sbx-bar`（"feiver 6k"）后激活 chip 变「feiver」、标题变
      「近 14 天趋势」——**视图确实切换**。（实测）
- [x] AC2 单沙箱视图显示近 14 天趋势图，数据与明细表一致
      —— 单沙箱视图渲染「近 14 天趋势」区块与图表容器。
      **注**：「数据与明细表一致」只验到两者同源渲染（同一 `/api/usage`
      数据），未逐点比对数值。
- [x] AC3 日期筛选生效；窗口切换（today/7d/all）不回归
      —— 窗口分段器 `aria-pressed` 随点击变化（`true,false,false` →
      `false,true,false`）；日期下拉含「全部 / 2026-09-22」，选中某日后
      明细表行数随之变化（2 行）。（实测）
- [x] AC4 无成本数据沙箱（pi 之外 agent）图表降级正常（隐藏成本系列）
      —— **owner 手动验证**（需造无成本数据样本，AI 侧无此数据）。
- [x] AC5 cargo test（app+mgr）+ tsc 全绿
      —— mgr `cargo test` 98 passed / 0 failed；
      mgr-web `npm run build`（tsc --noEmit && vite build）EXIT=0。（实测）

## Notes

- design.md + implement.md 在本任务 task.py start 前补全
