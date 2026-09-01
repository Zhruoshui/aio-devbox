# Design — 模型配置页重构（供应商卡片 + Kumo 重做 + 用量图表）

## 0. 决策记录（来自 AskUserQuestion）

| 决策 | 结论 | 依据 |
|------|------|------|
| 任务方式 | 建 Trellis 任务，走 prd/design/implement | 用户选「创建任务并规划」 |
| 模型配置路线 | **cc-switch 供应商卡片** | 用户选；参考 `UniversalProviderCard` 卡片网格 + 编辑器 |
| Anthropic 协议框 | **删除，协议决定端点** | 用户选；与 pi 一致，`api=anthropic-messages` 直接用 baseUrl |
| 用量图表 | **汇总卡 + 柱状 + 环图（纯前端）** | 用户选；不加按天时序，不动后端 usage |

## 1. 架构与边界

```
web/src/panes/models/                 app/src/routes/models/
┌─────────────────────────────┐        ┌─────────────────────────┐
│ ModelsPane (shell: tabs,    │        │ store.rs  (canonical)    │
│  state, fetch, dirty/save)  │  HTTP  │ mod.rs   (routes)        │
├─────────────────────────────┤ ◄────► ├ render/  (pi/opencode/   │
│ ProviderGrid (卡片网格)      │        │          claude/codex)   │
│ ProviderEditor (抽屉编辑器)  │        │ discover.rs test.rs      │
│ ModelTable (含 per-model协议)│        │ usage.rs                 │
│ AgentTabs (4 agent 绑定)    │        └─────────────────────────┘
│ UsageTab + charts.tsx       │
└─────────────────────────────┘
```

- **后端仅一处小改**：删除 provider `anthropic` 块（R1）。其余 API 契约不变，前端 4 个 tab 全部复用现有端点。
- **前端结构**：把 1954 行的 `ModelsPane.tsx` 拆成 `panes/models/` 目录（`App.tsx` 导入改为 `./panes/models`，导出 `ModelsPane` 保持同名）。拆分的依据：五个视图（网格/编辑器/4×agent/用量）各自有独立的状态与渲染，同文件将超过 2500 行（见 implement.md §2）。
- 每个子组件只收 **props + 回调**，不共享可变全局；解码仍只在 ModelsPane 边界做一次（沿用 cross-layer-thinking-guide「单一边界 owner」惯例）。

## 2. 数据模型变更（R1，后端）

### 2.1 `store.rs`
- `ProviderEntry` 删 `anthropic: Option<AnthropicBlock>` 字段；删 `AnthropicBlock` 结构体。
- `import_from_pi`（现 ~L407）删掉「api==anthropic-messages 时塞 anthropic 块」的逻辑。
- 旧数据兼容：serde 反序列化对未知字段默认忽略，所以含 `anthropic` 的旧 `models.json` 照常读入；下次 PUT 保存时该键自然丢弃。**无需迁移代码**。
- 测试：`import_from_pi` 测试里 `anthropic.is_some()` 断言改为 `is_none()`（或删除）。

### 2.2 `render/claude.rs`
- `apply_claude` 的 baseUrl 逻辑简化为 `let anthropic_base = &provider.base_url;`（删 anthropic 块优先）。
- `sample_config` 测试助手去掉 `anthropic` 参数；删 `anthropic_block_base_url_wins_when_present` / `anthropic_block_empty_falls_back_to_provider_base_url` 两个测试，替换为「协议=anthropic-messages 时 ANTHROPIC_BASE_URL==baseUrl」的既有覆盖（`apply_claude_preserves_*` 已断言 `== https://ai.aruoshui.com/v1`，保留即可）。
- 文件头注释同步更新（不再提 anthropic block）。

### 2.3 兼容矩阵（前端 `incompatibleReason`）
- claude 兼容条件从 `api==anthropic-messages || anthropic块存在` 收紧为 `api==anthropic-messages`。
- codex 不变（非 anthropic-messages）。
- 已保存的「openai 协议 + anthropic 块」老供应商在 claude 页签自动变不可选（显示原因文案），行为可预期。

## 3. 前端模块拆分（R3 基础设施）

```
web/src/panes/models/
  index.tsx        re-export ModelsPane（App.tsx 导入不变语义）
  ModelsPane.tsx   shell：tab 路由、fetchConfig/fetchAgents/fetchUsage、
                   dirty/save 状态、import、test 状态机、discover 状态
  types.ts         现有接口镜像 + 解码器（decodeConfig/decodeAgents/decodeUsage）
  ProviderGrid.tsx 卡片网格 + 单卡片（含 hover 动作、agent chips）
  ProviderEditor.tsx 抽屉编辑器（基本信息 / 高级 / 模型表格 / 绑定概览）
  ModelTable.tsx   模型表格（含 per-model 协议列 + discover/test pill）
  AgentTabs.tsx    4 个 agent 绑定页签 + apply 结果面板
  UsageTab.tsx     汇总卡 + 图表 + 明细表
  charts.tsx       SVG 柱状/环图组件（无依赖）
```

- `App.tsx` 仅改一行 import；`Sidebar.tsx` / `icons.tsx` / `types.ts`（ServiceType）不动。
- `ModelsPane.tsx` 保留全部数据获取与写回 handler；子组件通过 props 收窄回调。

## 4. 供应商库页签：卡片网格 + 编辑器抽屉（R2）

### 4.1 卡片网格（`ProviderGrid`）
- 布局：`display:grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 12px;`。
- 卡片 = Kumo LayerCard：`background:var(--surface); border-radius:var(--radius-lg); box-shadow:var(--elev-ring), var(--elev-raised)`；hover 提边（`--border`）+ 过渡 150ms。
- 卡片内容（上→下）：
  - 头部：协议徽标（pill，`--surface-warm` recessed，mono 小字 `anthropic-messages` 等）+ 名称（`--text-base`/600）+ hover 露出操作按钮（编辑/删除，icon-only，`aria-label`）。
  - Base URL：`--font-mono`、`--text-sm`、`--muted`、单行截断（title 提示全文）。
  - meta 行：`N 模型` + API Key 掩码（`sk-****abcd`）。
  - agent chips：被哪些 agent 绑定（`pi/opencode/Claude/Codex` 小 pill），点击跳 agent 页签。
- 空态：Kumo 空态（居中说明 + 主/次按钮「新增供应商」「从 pi 导入」）。

### 4.2 编辑器抽屉（`ProviderEditor`）
- 卡片点击 → 右侧覆盖抽屉：`position:absolute; right:0; top:0; bottom:0; width:min(440px, 100%); background:var(--surface); border-left:1px solid var(--border-soft); box-shadow:-2px 0 8px oklch(0 0 0/0.06)`；进出 250ms `--ease-standard`；reduced-motion 归 0。
- 抽屉头：供应商名（可改）+ 关闭 ×（icon-only aria-label）。
- 分节（每节 `--text-sm`/600 小节标题 + 紧邻 8px 间隙）：
  1. **基本信息**：名称、Base URL、协议（3 选）、API Key（password + 显隐切换）。
  2. **高级（折叠）**：Headers (JSON)、Compat (JSON) 两个 textarea（保留现有解析与校验）。
  3. **模型列表**（`ModelTable`，见 §5）：discover 拉取 / 新增模型按钮在节头。
  4. **绑定概览**：被此供应商绑定的 agent chips + 每 agent 当前模型，点击跳 agent 页签。
- 底部 save bar：脏标记 + 保存（主按钮）；保存成功 toast/内联提示，刷新拉回掩码键（沿用现有 PUT 契约）。

## 5. 模型表格（`ModelTable`）

- 列：`ID`（mono input）/ `协议`（select，空=继承 provider，可覆盖 `ModelEntry.api`）/ `显示名` / `推理`（checkbox）/ `上下文窗口` / `最大输出` / `成本 in/out/cacheR/cacheW`（number input）/ `检测`（pill）/ 删除。
- 每行 test pill 沿用现有 `TestStateMap`（key=`${providerId}:${modelId}`）与「provider 身份字段变更时重置」的 effect。
- discover 弹窗（`POST /api/models/discover`）从 ModelsPane 提为抽屉内模态，保持现有多候选/多形态解析契约，仅换 Kumo 视觉。
- 新增模型行时 `api` 留空（继承 provider 协议）。

## 6. Agent 页签（`AgentTabs`，R2 保留 + R3 重排）

- 4 页签（pi/opencode/Claude/Codex）沿用现有数据结构（`AgentsResponse` 实时回读、`incompatibleReason` 过滤、保存分配 + 生效 + 写入结果面板）。
- Kumo 化：卡片化表单（每字段 label + control 对齐 36px）、安装徽标（语义色：ok=`--success`/warn=`--warn`）、live 回读用 mono。
- 生效结果面板：written 文件列表 + backup 路径（mono）+ 错误列表（`--danger`），不改后端 apply 契约。

## 7. 用量页签（`UsageTab` + `charts.tsx`，R4）

### 7.1 数据（纯前端，从现有 `rows` 计算）
```
totalIn    = Σ r.in
totalOut   = Σ r.out
totalCache = Σ (r.cacheRead + r.cacheWrite)
totalCost  = Σ r.cost                       // 仅当存在 cost 字段
byModel    = Σ tokens per r.model           // in+out+cache，取 top 8，降序
costByAgent= Σ cost per r.agent             // 仅取 cost>0
```

### 7.2 汇总卡（4 张横排）
- 每张：`--text-xs` muted label + `--text-2xl`/600 数值（`font-variant-numeric: tabular-nums`）+ 小注。卡片样式同 §4.1。

### 7.3 水平柱状图（`charts.tsx` `TokenBars`）
- 按模型 token 总量，`display:flex` 纵列 + 条宽 % of max；条色 = Kumo 分类色按序循环；条尾 `fmtTokens` 数值标签 + 模型名（不只靠颜色）。
- 空数据：`--muted` 文案占位。

### 7.4 成本环图（`charts.tsx` `CostDonut`）
- SVG `<circle>` stroke-dasharray 分段；段色 = 分类色按 agent 分配；中心显示总成本；右侧图例（agent 名 + `fmtCost` + 占比 %）。
- 无 cost 数据（全部行无 `cost`）→ 整卡隐藏，不占位报错。
- `aria-label` 描述环图含义；图例保证 color 之外可辨。

### 7.5 明细表
- 现有表（agent/provider/model/in/out/cache/cost/合计行）保留，重排为 Kumo 表格（表头 `--text-xs` muted、右对齐数值、`--border-soft` 分隔、合计行 `--surface-warm` 底）。

## 8. Kumo 视觉规范（R3 全 pane）

- 页签栏：`--surface-warm` recessed 底 + 下边 hairline；active 用 `--accent` 2px 下边线 + weight 600，hover `--fg`，focus-visible 2px ring。
- 按钮：普通动作 secondary（`--surface` + ring，hover 提边），唯一主按钮 primary（`--accent`/`--accent-hover`/`--accent-on`）；destructive 仅删除。
- 表单控件：36px 高、`--radius-md` 6px、focus ring 2px `--focus-ring`；placeholder 不替代 label。
- 字体：Inter（已加载）；代码/标识/数值用 `--font-mono`（0.9em）。
- 所有颜色只经 token 层，禁止 `dark:` 手动变体；`prefers-reduced-motion` 已由 `--motion-*` 归 0 覆盖。
- `mc-*` 旧样式块整体替换为新的语义类（`.ml-*` 前缀，避免与旧样式混淆），删除 `mc-` 块。

## 9. 兼容性与回滚

- **回滚点**：`git revert` 即可；后端改动是纯删除（anthropic 块），前端是独立模块目录，互不影响。
- 旧数据：含 `anthropic` 的 models.json 读入后字段被忽略（R1）；已绑定「openai+anthropic块」的 claude 分配在 UI 变不可选，apply 前 UI 已拦截。
- 现有行为保留：掩码回传语义、备份×3 + 回读校验 + 失败还原、usage 30s 缓存、discover 20s deadline、test 20s 探测——全部不动。
- 图表无新依赖：手写 SVG/div，避开 chart lib 的 React 18 版本耦合与包体积。

## 10. 边界与不做

- 不做：按天时序趋势图（后端 usage 加时间轴）、cc-switch 的拖拽排序/故障转移/用量脚本、pi 的模型目录元数据下载、供应商预置模板库。
- 不做：其他 pane 的 Kumo 化（仅本页）。
- 不做：`ModelEntry.headers` / `ModelEntry.compat` 的 UI 暴露（保留后端字段，后续任务再说）。
