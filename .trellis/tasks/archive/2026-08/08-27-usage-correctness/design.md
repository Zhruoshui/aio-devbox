# Design - 用量统计数据正确性 + 表格对齐修复

> prd.md 见同目录。本文落实成本补算的匹配规则、cache 单价、零值行过滤、
> 表格对齐修复。审计基线来自容器真实日志(见 §0)。

## 0. 审计基线(容器真实日志 ground truth)

对 `~/.pi/agent/sessions`、`~/.local/share/opencode/opencode.db` 的真实抽样:

- **opencode**:`message` 9 条 assistant 行,**全部 `cost=0`**(字段存在但恒为 0,
  非真实成本)。canonical 有 cost 配置(aruoshui-openai-completions 的
  deepseek-v4-flash: in/out/cacheR/cacheW = 0.14/0.28/0.0028/0.0)。
- **pi**:23 条带 usage 的 assistant,**`usage.cost.total` 全为 0**(cost 字段存在
  但恒 0)。
- **结论**:prd.md 的前提「pi/opencode 日志已有 cost -> 用日志值」按字面会让所有
  行 cost 停在 0。真实情况是这些日志的 cost 不可信(恒 0)。成本补算(选项 A)
  必须把「日志 cost == 0 或缺失」当作「无 cost」走补算,**仅当日志 cost > 0 时**
  才信任日志值(避免双计)。
- **零值行存在**(如 mimo-v2.5-free:`in=out=cacheRead=cacheWrite=0`),是噪声。
- **claude/codex 行**:`provider=None`(scan 不做 canonical join),且日志模型 id
  可能带日期后缀(如 `claude-sonnet-4-20250514`)与 canonical 模型 id 不精确相等。
  本环境无 claude/codex 会话日志,补算匹配按设计容错(见 §2)。

## 1. 成本单位约定(canonical `provider.models[].cost`)

- **约定:canonical `cost` 各项为 `USD / 1M tokens`**($/百万-token)。依据:
  - 用户现有配置 deepseek-v4-flash `input=0.14`、gpt-5.6-sol `input=5.0` 与
    行业真实定价($0.14/M、$5/M)一致;
  - models.dev(选项 A 后续来源)原生也是 $/M;
  - 若按 $/token 则 0.14/token = $140k/M,荒谬。
- **补算公式**:`cost_usd = (in/1e6)*input + (out/1e6)*output + (cacheRead/1e6)*cacheRead + (cacheWrite/1e6)*cacheWrite`。
  cacheRead 单价 < input < cacheWrite 单价(缓存读比新输入便宜、缓存写比新输入贵),
  分项计费,不混算(见 prd.md)。
- **UI 单位标注**:`ModelTable.tsx` 的 cost 列头加 `($/M)` 后缀,4 个子输入 in/out/
  cacheR/cacheW 各自的列头补 `($/M)`。

## 2. cost 来源优先级 + 匹配规则(pure fn `backfill_cost`)

每条 `UsageRow` 在 merge 后、返回前过一遍:

1. **日志 cost > 0** -> 用日志值,**不补算**(避免双计)。
2. **日志 cost == 0 或 None** -> 补算。按顺序找 canonical `CostEntry`:
   a. row.provider 已知 且 在 canonical 中 且 该 provider 的 models 含**精确** model id 且有 cost -> 用;
   b. 跨所有 provider 精确匹配 model id,**首个有 cost 配置的** -> 用(opencode 的
      `providerID="opencode"` 不在 canonical -> 落到这步;`aruoshui-openai-completions`
      命中 §2a 直接用);
   c. **模糊匹配(收紧版,容器对账修正)**:仅「row model = canonical id + **版本/日期式
      后缀**」(后缀形如 `-\d[\d.]*`,如 `-20250514`/`-4`/`-4.5`)。**字母变体后缀
      (`-free`/`-vision-exp`)是不同模型,拒绝匹配**——对账发现
      `deepseek-v4-flash-free` 被误按 `deepseek-v4-flash` 费率计费;反向前缀
      (短 row id 匹配长 canonical id,如 `gpt` vs `gpt-5`)同样拒绝。取最长
      canonical id(最具体)首个有 cost 的。
   d. 都不中 -> 保留日志值(None 或 0;日志 0 视为真实免费,不改为 None)。
3. 命中 §2b/2c 时,若 row.provider 为 None 且匹配到的 provider 唯一,**顺带回填
   row.provider**(让明细表 provider 列对 claude/codex 也有值,非必要但友好;
   多个候选命中时不回填,避免误标)。

补算只在「日志无可用 cost」时触发,且命中的 cost 是 canonical 已配置的;
未配置 cost 的模型行 cost 仍 None。**不触碰日志 cost > 0 的行** -> 无双计风险。

## 3. 零值行过滤(scan 层)

`merge_row` 后,丢弃 `in+out+cacheRead+cacheWrite == 0` 的桶(纯噪声:零用量
= 无真实使用)。在 handler 的 `rows` 收集阶段做一次 filter,不改各 scan 的输出
契约(保持单测不变)。pi 的 `mimo-v2.5-free (0,0,0,0)` 等会被滤掉。

## 4. 前端表格对齐(UsageTab.tsx + styles.css)

- **cache 拆两列**:明细表把「Cache」单列拆为 **Cache Read** / **Cache Write** 两列
  (prd.md AC:「cacheRead/cacheWrite 列在明细表正确显示,不与 input 混」)。合计行
  同步两列。汇总卡「缓存」仍显示 read+write 合计(汇总卡不受影响)。
- **数字列右对齐**:已有 `.ml-num { text-align: right; tabular-nums }`,扩展到
  cache 两列与 cost 列(已右对齐,核对)。
- **表头粘性**:`.ml-usage-table-card` 内 `<table>` 包一层 `overflow-x:auto` 容器,
  `thead th` `position: sticky; top:0; z-index:1`(竖向滚动时表头不跑出)。
- **长模型名截断**:model/provider 单元格 `max-width` + `text-overflow: ellipsis`
  + `title=全名`。
- **窄宽不溢出**:表格容器 `overflow-x:auto`,`table { min-width: ... }` 保证列不被
  压成换行;汇总卡 `ml-stats` 已是 `auto-fit` 等宽;柱状图 label 固定宽 + ellipsis;
  环图 legend 用 grid 三列(色块/标签/值+占比)对齐。
- **hasCost 语义修正**:`hasCost = rows.any(cost.is_some())`(有任一行配出 cost ->
  显示 cost 列);donut 仅当 `rows.any(cost > 0)` 才显示(全 0 不画空环)。
  当前 bug:`hasCost = cost !== undefined`,opencode 全 0 也触发 cost 列+空环 -> 修。

## 5. 不做

- 不动后端时间序列(仍是聚合行,不加按天)。不引入 chart 库。
- 不改各 scan 的日志解析契约(字段路径已 VERIFIED,见 usage.rs 头注释)。
- 不为 claude/codex 单独 join canonical provider(靠 §2c 模糊匹配 + §2d 回填)。
- 不缓存补算结果随 cache 行为变化(canonical 变了要 `?refresh=1` 重建,沿用现有 TTL)。

## 6. 兼容矩阵

| 行来源 | 日志 cost | canonical cost 配置 | 结果 |
| --- | --- | --- | --- |
| pi/opencode | >0 | 任意 | 日志值(信任) |
| pi/opencode | 0 或无 | 已配(精确/版本式模糊命中) | 补算(§2 公式) |
| pi/opencode | 0 | 未配/不命中 | 保留 0(真实免费,如 glm-5.3-flash/deepseek-v4-flash-free) |
| pi/opencode | None | 未配/不命中 | None |
| claude/codex | 无(不携带) | 已配(精确/版本式模糊命中) | 补算 |
| claude/codex | 无 | 未配/不命中 | None |
| 字母变体(`-free`/`-exp` 后缀) | 任意 | 仅模糊可中 | 不匹配(不同模型,保留日志值) |
| 全零 token 行 | - | - | §3 滤掉 |

## 7. 测试策略

- `backfill_cost` 单测:日志>0 保留;0/None + (provider,model)精确命中;0/None +
  仅 model 精确命中(provider 不在 canonical);模糊前缀命中;都不中 -> None;cache
  分项计费(cacheRead/cacheWrite 用各自单价,不混 input)。
- 零值行过滤单测(merge 后全 0 的桶被丢)。
- 现有 scan 单测保持绿(补算在 merge 之后,不改 scan 契约)。
- 前端:`npm run build`;表格对齐目测(容器实测:窄宽不溢出、cache 两列、表头粘性)。
- 容器对账:取一条真实 opencode 会话,手算 `in/1e6*0.14 + out/1e6*0.28 + cacheRead/1e6*0.0028`
  与 `/api/models/usage?window=all&refresh=1` 返回该行的 cost 对账。
