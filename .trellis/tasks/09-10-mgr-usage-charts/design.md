# S4 设计：用量图表——分沙箱条形 + 按天趋势（D5）

## 0. 现状事实（勘察结论）

- **app usage.rs**：`GET /api/models/usage?window=today|7d|all` → `UsageResponse{rows, generatedAt}`，rows 是 `(agent, provider, model)` 聚合（跨日）。四扫描器（pi/claude/codex jsonl + opencode SQLite）**逐记录已解析 `t`（secs 时间戳）**，但聚合桶 `buckets` 丢掉时间维度——byDay 必须让扫描器把 per-record 时间带出。
- `days_to_ymd(days)`（usage.rs:241）已存在（无 chrono 的 civil 转换），`parse_timestamp_secs` 已解析 pi/claude/codex 记录时间、opencode 用 `time_created`(ms)。
- **mgr usage.rs**：`GET /api/usage?window=` → `{"sandboxes": [{name, error, usage|null}]}`——**纯拼接透传**（fetch_one 取 app 原始 JSON，不重算）。R1「纯拼接，不重算」= mgr 从各 usage.rows sum 出每沙箱聚合，不重新解析会话。
- **前端 UsagePage.tsx**：已有 TokenBars/CostDonut/chip 选择器（勘察锚点）。样式在 charts.tsx（TokenBars/CostDonut 是自研 SVG）。

## 1. app 端 byDay（R2，核心）

### 1.1 扫描器签名改造

四个 scan_* 返回类型统一改为携带 day 聚合：

```rust
/// 一个扫描器的全部产出：聚合行（现 rows，跨日）+ 按日明细（S4）。
pub struct UsageScan {
    pub rows: Vec<UsageRow>,
    pub by_day: Vec<DayUsage>,
}
pub struct DayUsage {
    pub date: String,        // "YYYY-MM-DD" (UTC, days_to_ymd)
    pub agent: String,
    pub model: String,
    pub r#in: u64, pub out: u64,
    #[serde(rename="cacheRead")] pub cache_read: u64,
    #[serde(rename="cacheWrite")] pub cache_write: u64,
    #[serde(skip_serializing_if="Option::is_none")] pub cost: Option<f64>,
}
```

- 每扫描器在**现有逐记录循环内**（拿到 `t` 之后、cutoff 过滤之后）额外累加
  `day_buckets: BTreeMap<(i64 /*day=t/86400*/, String /*model*/), DayUsage>`——
  day 取 `t / 86400`（UTC 自然日），agent 固定为该扫描器的 agent，model 与
  现 rows 同源，cost 同规则（pi/opencode 有、claude/codex 无）。
- 循环结束后 day_buckets 转 `Vec<DayUsage>`，date = `day_label(day)`（新纯函数，
  复用 `days_to_ymd`：`{:04}-{:02}-{:02}`）。provider join 与 rows 一致（find_provider_for_model）。
- `scan_pi/scan_claude/scan_codex/scan_opencode` 返回类型改 `UsageScan`；内部
  现有 rows 逻辑**一字不动**（仅包一层 + 并行 day 桶）。

### 1.2 handler 合并 + 14 天序列

- `UsageResponse` 增 `by_day: Vec<DayUsage>`。
- handler 内：把四扫描器 by_day 按 `(date, agent, model)` 合并（BTreeMap 累加，
  同 merge_row 语义）；then **补零成满 14 天序列**（`[now_day-13 … now_day]`，
  无数据显示 date + 全零行）→ 前端趋势图直接画无需补零。
- **与 window 参数解耦**：by_day 恒定最近 14 天（R2「近 14 天」）；今天的
  today 窗口也同时返回 by_day（趋势图不受窗口切换影响）。
- 缓存：CacheEntry 增 by_day；`usage` handler 缓存命中同样返回。
- cost：仅 pi 有成本（AC4 降级）；某天某 agent 无 cost 时该 day 的
  cost=None → 前端隐藏该系列/该段。

### 1.3 边界

- 时间戳缺失：parse_timestamp_secs 失败降级 file mtime（现有行为），by_day
  同样用该降级值——一致性保持。
- day 桶不 join provider 到 DayUsage（前端趋势按 agent+model，provider 在
  明细行已有；AC2 数据与明细表一致的成本最低路径）。

## 2. mgr 端每沙箱聚合（R1）

- `GET /api/usage` 响应增 `"totals": [{name, in, out, cost}]`。
- 纯拼接：对每个非 error entry 的 `usage.rows`（app 已算好），sum `in/out`
  与 `cost`（cost Option → 0 当 None）；error entry 的 totals 项 `in/out/cost=0`。
- 实现：`assemble_totals(entries: &[Value]) -> Vec<Value>` 纯函数（可单测），
  usage handler 在装配 entries 后调用。
- **是否进缓存**：totals 由已缓存的 entries 派生，fan_out 命中的缓存直接算，
  不额外探测。

## 3. 前端 UsagePage

### 3.1 合计视图分沙箱条形图（R1，AC1）

- 新图组件 `SandboxBars`（charts.tsx 同风格 SVG）：横向条形，每沙箱一条，
  长度=总 token（in+out），颜色区分有/无成本；**点击条 → 切到该沙箱单视图**
  （UsagePage 现有 sandbox 选择逻辑）。
- 数据源 `GET /api/usage.totals`（新字段）。无 totals（旧 mgr）→ 前端从
  entries 内联降级计算或隐藏。

### 3.2 单沙箱按天趋势（R2/R3，AC2/AC4）

- 新图组件 `DayTrend`（charts.tsx 同风格）：柱状双系列（token in/out，近 14
  天）+ 成本折线叠加（有成本才显示，AC4）；X 轴日期，hover 显示数值。
- 数据源 app by_day（经 mgr 透传——`GET /api/usage` 的 entries[].usage.byDay，
  前端解码 camelCase）。旧 app 无 byDay → 图表隐藏。

### 3.3 明细表日期列 + 筛选（R4，AC3）

- 明细表行保持 (agent/model) 聚合；增「日期」列头 + 日期选择器（近 14 天
  下拉 + 「全部」）。
- 选中某天：明细表**过滤**为「该日有耗用的 agent/model 行」+ 该行的
  byDay 值显示在日期列（无 = —）；选「全部」= 现状。
- 实现：日期选择器 state → 从 by_day 构建当日集合过滤 rows。
- 现有窗口 chip（today/7d/all）不回归：仍控制 rows 的时间窗口；日期列
  独立于 chip。

## 4. API 契约

- `GET /api/models/usage`（app）增 `byDay: [DayUsage]`（camelCase: date/agent/
  model/in/out/cacheRead/cacheWrite/cost）。
- `GET /api/usage`（mgr）增 `totals: [{name, in, out, cost}]`；`entries[].usage`
  透传 app 原文（含 byDay）。

## 5. 兼容与回滚

- 新字段均为追加：旧前端忽略 byDay/totals；新前端读旧后端 → 字段缺失
  前端降级（隐藏图/推算）。双向安全。
- 扫描器返回类型改 `UsageScan`：**内部** API，app 内唯一调用点是 usage
  handler 与测试；影响面受控。
- 回滚 revert 即可，无数据迁移。

## 6. 测试锚点

- app：day_label 纯函数；DayUsage 聚合正确性（fixture jsonl 两日分摊）；
  满 14 天补零；cost=None 降级。
- mgr：assemble_totals（有/无 error entry、cost None→0）。
- 前端：tsc + build；图表组件数据映射。