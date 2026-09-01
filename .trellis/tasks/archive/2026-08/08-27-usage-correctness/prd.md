# 用量统计数据正确性 + 表格对齐修复

> 父任务 08-27-models-config-v2 的 R5。独立、低耦合,可先行。

## Goal

核查 usage 各 agent 源的 cacheRead/cacheWrite/cost 归账是否与原始日志一致;修复前端
表格字段不对齐;必要时补全 claude/codex 的成本归账(其日志不携带 cost)。

## Requirements

### 缓存归账核查(各 agent 源)

- claude(`~/.claude/projects/**/*.jsonl`):`message.usage.cache_read_input_tokens` -> cacheRead、
  `cache_creation_input_tokens` -> cacheWrite。**核查**:是否每条 assistant 消息的 usage 都
  被聚合(含 compaction/branch_summary 跨压缩条目,保证单调)。
- codex(`~/.codex/sessions/**/*.jsonl`):`token_count` 事件 `total_token_usage.cached_input_tokens`
  -> cacheRead;codex 无 cacheWrite(核查是否漏计某字段)。
- opencode(opencode.db):`tokens.cache.{read,write}`。
- pi(sessions jsonl):`message.usage.cacheRead/cacheWrite`。
- 抽样手算:取一条真实会话日志,手算 token/cache/cost,与 `/api/models/usage?window=all`
  返回的对应行对账;差则为 bug,修。

### 成本归账(关键核查点)

- 现状:cost 仅来自 agent 日志(pi `usage.cost.total`、opencode `data.cost`);claude/codex
  日志不带 cost -> 其行 `cost=None`,成本环图与汇总卡不显示它们的成本。
- **决策(已定:选项 A)**:用 canonical 模型 cost 配置(provider.models[].cost)× token
  用量,为日志无 cost 的行(claude/codex)**补算成本**。日志已有 cost 的行(pi/opencode)
  用日志值,避免双计。设计阶段落实匹配规则与 cache 单价,记录于 design.md。
- cache 计费:若补算成本,cacheRead 用 cacheRead 单价、cacheWrite 用 cacheWrite 单价
  (通常 cacheRead < input < cacheWrite),不得全按 input 单价计。

### 前端对齐修复

- 明细表(ml-table)与汇总表:列宽统一、表头与单元格垂直/水平对齐、数字列右对齐
  (`.ml-num`)、表头粘性可选、长模型名截断 + title。
- 卡片图布局:汇总卡 4 张等宽且数值基线对齐;柱状图条目对齐;环图图例行对齐。
- 响应式:窄宽下表格不溢出(横向滚动或折叠)。

## Acceptance Criteria

- [x] 抽样对账:至少 2 个 agent 的真实会话,手算 token/cache/cost 与 API 返回一致(或
  记录差异并修复)。(opencode + pi 各 3 行手算精确一致;对账中发现并修复模糊匹配
  误报:`-free` 变体不再按基础模型计费)
- [x] 成本归账策略落地并在 spec 记录;无论 A/B,实现与策略一致。(选项 A:$/M 单位、
  日志>0 信任/0 或 None 补算、精确→版本式模糊匹配、cache 分项计价)
- [x] 明细表/汇总表字段对齐修复(截图或目测);窄宽不溢出。(cache 拆读/写两列、
  粘性表头 + 横向滚动容器、长名截断 + title、legend grid 对齐)
- [x] cacheRead/cacheWrite 列在明细表正确显示(不是 0 乱填、不与 input 混)。
- [x] `cargo test` 绿(补成本补算若有,加单测覆盖);`npm run build` 干净。(166 绿,
  9 个补算/过滤新测试)

## Notes

- 不动后端时间序列(仍是聚合行,不加按天)。
- 不引入 chart 库。
- 若选成本补算(选项 A),复用 canonical `provider.models[].cost` 与 `usage` 的
  (agent,provider,model) 行匹配;模型匹配不到 cost 配置的行 cost 仍 None(不臆造)。
