# Implement - 用量统计数据正确性 + 表格对齐修复

> 前置:读 `prd.md`、`design.md` §0-§7。改动顺序:后端补算+零值过滤 -> 后端测试绿 ->
> 前端表格对齐 + hasCost 修正 -> 视觉/i18n -> 容器对账 -> 收尾。

## 1. 后端成本补算(usage.rs)

- [x] pure fn `backfill_cost(rows: &mut [UsageRow], canonical: &CanonicalConfig)`:
      按 design §2 优先级(日志>0 保留;0/None 走 canonical 匹配 a→b→c→d);
      命中 §2b/2c 且 provider None 且候选唯一时回填 provider。
- [x] cost 计算 helper:`price(tokens, per_m)` = tokens/1e6 × per_m;分项
      in/out/cacheRead/cacheWrite 各用各的单价(design §1)。
- [x] handler:`merge_row` 收集完成后、零值过滤后、排序前调用 `backfill_cost`
      (canonical 已在 handler 读到,直接传)。
- [x] 零值过滤:`rows.retain(|r| r.in + r.out + r.cache_read + r.cache_write > 0)`
      (design §3,不动 scan 契约)。

## 2. 后端测试

- [x] backfill_cost:日志>0 保留(不覆盖);==0 + provider 精确命中;None + 跨
      provider model 精确命中;模糊前缀命中(双向);不命中 -> None;cache 分项
      计费数值断言(cacheRead≠input 单价);provider 回填(唯一候选/多候选不回填)。
- [x] 零值过滤:全 0 桶被丢、非 0 桶保留。
- [x] 现有 usage 测试全绿(throwaway rust 容器:`docker run --rm -v $PWD/app:/app
      -v aio-cargo-registry:/usr/local/cargo/registry -v aio-cargo-cache:/usr/local/cargo
      -w /app rust:1-bookworm cargo test models`)。

## 3. 前端表格对齐(UsageTab.tsx + styles.css + ModelTable.tsx)

- [x] cache 拆两列:表头 Cache Read / Cache Write(新 i18n 键 mcUsageColCacheR/
      mcUsageColCacheW,zh+en);明细行与合计行同步;移除现有 title 合计 tooltip。
- [x] hasCost 修正:cost 列显示条件 = any(cost.is_some());donut 显示条件 =
      any(cost > 0)(design §4)。
- [x] 表格容器:`.ml-usage-table-card` 内包 `overflow-x:auto` + `thead th` sticky
      (top:0);table min-width 防压扁。
- [x] 长 model/provider 单元格:max-width + ellipsis + title 全名。
- [x] 柱状图 label / 环图 legend 对齐核查(label 固定宽 ellipsis;legend grid 列对齐)。
- [x] `ModelTable.tsx` cost 列头加 `($/M)`(mcCost 文案后缀或新键),子列头
      in/out/cacheR/cacheW 同步标注。

## 4. 验证(质量门)

- [x] `cargo test models` 全绿。
- [x] `cd web && npm run build` 干净。
- [x] 容器对账(design §7):重建 app 镜像 + force-recreate(netns 侧车一起);
      取真实 opencode 会话手算 cost 与 `?window=all&refresh=1` 该行对账一致;
      pi 行 cost 从 0 变为补算值或 None;零值行消失;窄宽目测不溢出、表头粘性、
      cache 两列。
- [x] 对照 prd AC 逐项打勾;`trellis-check` 复查。

## 5. 收尾

- [x] 更新 `.trellis/spec/backend/model-config-guide.md` usage 段:成本单位 $/M
      约定、补算优先级与匹配规则、零值行过滤;前端表格列说明。
- [x] journal-1.md 当日进展。
- [x] commit(`feat: 用量成本补算($/M 约定)+ cache 拆列 + 表格对齐修复`);
      归档子任务;回父任务标记 R5。
