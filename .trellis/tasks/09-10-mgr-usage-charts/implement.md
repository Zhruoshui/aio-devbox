# S4 执行计划

前置：design.md §1-§6 为实现依据。顺序：app usage（byDay 核心）→ mgr 聚合 → 前端。每步独立编译。

## Step 1 app：usage.rs 结构（byDay 基础设施）

- [ ] 新增 `DayUsage` 结构 + `UsageScan` 结构（design §1.1）
- [ ] 新增纯函数 `day_label(day: i64) -> String`（`{:04}-{:02}-{:02}`，复用 days_to_ymd）
- [ ] 新增 `UsageResponse.by_day: Vec<DayUsage>` 字段（serde `byDay` camelCase）
- [ ] cache CacheEntry 增 by_day（clone 传播）
- 验证：`cargo build -p aio-app`

## Step 2 app：四扫描器产出 by_day（R2）

- [ ] scan_pi/scan_claude/scan_codex：返回类型改 `UsageScan`；逐记录循环内
      加 `day_buckets (day, model) -> DayUsage` 累加（t 已在手）+ provider join
- [ ] scan_opencode：同（t 从 time_created ms，day = ms/86400000）
- [ ] handler：合并四扫描器 by_day + 补零满 14 天（`now_day-13..=now_day`）→
      UsageResponse.by_day
- [ ] 单测：DayUsage 聚合 fixture（两日 jsonl 分摊）、补零序列、cost None 保留
- 验证：`cargo test -p aio-app models::usage`

## Step 3 mgr：每沙箱 totals（R1）

- [ ] `assemble_totals(entries: &[Value]) -> Vec<Value>` 纯函数：非 error entry
      sum usage.rows 的 in/out/cost（cost None→0）；error entry 全 0
- [ ] `GET /api/usage` 响应增 `"totals"`（在 entries 装配后调用）
- [ ] 单测：assemble_totals 有/无 error、cost 缺失
- 验证：`cargo test -p aio-mgr usage::`

## Step 4 前端：类型 + API

- [ ] types.ts：`DayUsage`、`UsageFanout` 增 totals、`SandboxUsage` 增 byDay
- [ ] 无新增 API（复用 getUsage；后端加字段）
- 验证：tsc

## Step 5 前端：分沙箱条形图（R1/AC1）

- [ ] charts.tsx（或 UsagePage 内）新增 `SandboxBars` SVG：横向条（token 和），
      有/无成本颜色分别；点击 → 切单沙箱视图
- [ ] 数据源 getUsage response totals；旧 mgr 无 totals → 前端从 entries 推算
- 验证：tsc + 手工

## Step 6 前端：按天趋势图（R2/R3/AC2/AC4）

- [ ] charts.tsx 新增 `DayTrend` SVG：柱状双系列（in/out token）+ 成本折线
      （有 cost 才画）；X 轴 14 天
- [ ] 数据源 entries[].usage.byDay；旧 app 无 byDay → 隐藏图
- 验证：tsc + 手工

## Step 7 前端：明细表日期列 + 筛选（R4/AC3）

- [ ] 日期选择器（14 天下拉 + 全部）+ 日期列显示选中日用量（byDay 过滤）
- [ ] 窗口 chip 不回归
- 验证：tsc + 手工

## Step 8 质量检查

- [ ] `cargo test -p aio-app -p aio-mgr` 全绿
- [ ] `cd mgr-web && npx tsc --noEmit && npm run build`
- [ ] 手工链：合计分沙箱、单沙箱趋势、日期筛选、cost 降级

## Step 9 收尾

- [ ] spec 更新（model-config-guide usage 段 + api-contracts usage 契约）
- [ ] 提交 + journal

## 回滚点

- 每 Step 可独立编译。新字段追加双向兼容；扫描器返回类型为 app 内部 API，
  改动局限在 usage.rs。revert 即回滚。