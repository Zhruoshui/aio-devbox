# Design — pi/opencode 页签 live 配置管理 + 供应商模型列表复用

> R2+R3。前置 R1 已落地:ModelPicker.tsx(无状态,待接线)、catalog 路由。
> 代码现状引用见 Explore 摸底(2026-08-27),关键锚点:
> `mod.rs:292-343 read_live`、`store.rs:698-754 import_from_pi`、
> `render/pi.rs`、`render/opencode.rs`、`AgentTabs.tsx`、`PresetList.tsx`(UI 范式)。

## 0. 总体思路

pi/opencode 是 incremental agent:原生配置文件里 **N 个 provider 共存**,
canonical `agents.{pi,opencode}` 只是「默认指向谁」。本设计把页签从
「单行 readback + select 下拉」升级为三块:

```
┌─ pi 页签 ──────────────────────────────────────┐
│ [已安装] [live 与分配一致 ✓]                     │
│ ── 分配(默认)─────────────────────────────── │
│  供应商 [select▾]   模型 [ModelPicker 触发器▾]   │
│  [保存] [应用]  (written/backup/errors 面板)     │
│ ── 现有配置(live @ ~/.pi/agent/models.json)── │
│  ▸ deepseek  openai-completions  api.deepseek.com  6 模型 [当前默认] │
│      [同步到供应商库] [编辑] [删除]              │
│  ▸ kimi      …                                  │
└────────────────────────────────────────────────┘
```

设计原则:**canonical 仍是 SSOT**。live 编辑/删除/同步是「吸收用户在 agent
侧手改」的通道,不是第二套配置源;分配/应用流程完全不动。

## 1. 后端:live 回读扩展(`GET /api/models/agents`)

`AgentStatus { installed, bin, live }` 结构不变,live `Value` 内容扩充。

**pi**(现 `mod.rs:294-307` 只读 settings.json):

```jsonc
{
  "provider": "deepseek",        // settings.json defaultProvider(现有,保留)
  "model": "deepseek-v4",        // settings.json defaultModel(现有,保留)
  "providers": [                  // 新增:models.json providers 摘要
    { "id": "deepseek", "name": "DeepSeek", "api": "openai-completions",
      "baseUrl": "https://api.deepseek.com/v1",
      "models": ["deepseek-v4", "deepseek-v4-flash"] }
  ]
}
```

**opencode**(现只读顶层 `model`):同样新增 `providers[]`,`name` 取
fragment.name、`baseUrl` 取 options.baseURL、`api` 由 npm 包名反推
(含 "anthropic" → anthropic-messages,否则 openai-completions;npm 缺失则省略
api)、`models` 取 fragment.models 对象的 key。

实现要点:

- **容错提取,不用整文件结构体反序列化**:providers 摘要走
  `serde_json::Value` 逐节点 best-effort(id=key,name/api/baseUrl/models
  任缺则省略或空),单个节点畸形只跳过该节点,不让整个 live 失败。
  (与 `import_from_pi` 的整文件 `CanonicalConfig` 解析是两条路:摘要要
  永不失败,同步要严格。)
- 两文件独立读取:pi 的 models.json 损坏 → `providers` 缺省 `[]`,settings
  默认值照常;两者都缺/损坏 → `live: null`(维持 read_file_optional 语义,
  不产生 HTTP 错误)。
- opencode 单文件:json5 解析失败/缺失 → `live: null`。
- `liveReadbackText`(types.ts:456)claude/codex 分支不动;pi/opencode 在
  AgentTabs 不再走单行文本,改由 live 列表 + 默认徽标表达。

## 2. 后端:live 管理路由(3 条新路由)

| Route | 语义 | 响应 |
| --- | --- | --- |
| `PUT /api/models/agents/:agent/provider/:id` | **字段级** patch live provider 节点 | `ApplyResult` |
| `DELETE /api/models/agents/:agent/provider/:id` | 删除 live provider 节点 | `ApplyResult` |
| `POST /api/models/agents/:agent/sync` | 吸收 live provider 进 canonical 库 | `{imported[], skipped[]}` |

约束:

- `:agent` 复用 render/common.rs `Agent` 解析;非 pi/opencode(claude/codex/
  未知)→ 400。claude/codex 的 preset 体系不走本路由。
- **PUT body = ProviderPatch `{name?, baseUrl?, apiKey?, api?}`**(serde
  camelCase、全 Option;`apiKey: ""` 表示清除,与 canonical masking 约定一致)。
  语义是**合并进现有节点的这几个键**,不是整节点替换——否则会砸掉编辑表单
  没暴露的 models/cost 等字段。
- **DELETE 级联清理悬空默认**:
  - pi:settings.json 的 `defaultProvider == id` → 同时删 settings 的
    `defaultProvider`/`defaultModel` 两键(键级,其他键不动);
  - opencode:顶层 `model` 以 `"<id>/"` 开头 → 同时删 `model` 键。
- 文件缺失/节点不存在 → 仍 200 + `ApplyResult{ok:false, errors:[...]}`,与
  apply 的 best-effort 通道一致(前端复用现成结果面板)。
- sync body `{id?}`:省略 = 全部同步(对齐 import_pi);给 id = 单个。
  幂等:已在 canonical → 入 `skipped`。持 `models_lock` 写 canonical。

### 写回函数落位(不新开写文件路径)

render/pi.rs:`edit_pi_provider(home, id, &ProviderPatch)` /
`delete_pi_provider(home, id)`;render/opencode.rs 同名 opencode 版。内部
沿用 read(Value 容错)→ 键级操作 → `backup_write_verify_json` 备份/原子/
回读校验/失败回滚管线。pi 侧 settings 键删除复用同一管线。

opencode 写回 = json5 读 → pretty JSON 写,**注释会丢**——与 `apply_opencode`
现状一致(spec 已记录),PRD「json5 注释文件读写无损」按「容错读 + 其他键
保留」执行,注释保留不在本任务范围(见 §6 取舍)。

## 3. 后端:sync 适配器

- **pi**:重构 `import_from_pi` 内部抽出 `map_pi_provider(key, value)`(sanitize_id
  + name 回填逻辑原样),全量与单个共用;路由层单 id 时若 key 不存在 → 404。
- **opencode**:`store.rs` 新增 `import_from_opencode(path, current) -> ImportResult`,
  fragment → `ProviderEntry` 反向适配:
  - `options.baseURL` 缺失 → 该 provider 入 skipped(理由字段化在 message,
    ImportResult 结构不变);
  - `api`:npm 含 "anthropic" → anthropic-messages,否则 openai-completions
    (render_opencode 正向映射的逆);
  - models:fragment.models 对象 key → `ModelEntry{id, name: value.name}`;
  - `options.apiKey`/`options.headers` 直映射。
- 两个 import 共用错误枚举:缺失 → NotFound(路由 404),损坏 → Corrupt(422)。

## 4. 前端

### 4.1 types.ts

- `LiveProviderSummary {id; name?; api?; baseUrl?; models: string[]}`;
  `AgentLive` 增 `providers?: LiveProviderSummary[]`。
- `decodeAgents` 容错透传(逐项 decode,畸形项跳过)。
- 新 helper `liveMatchState(agent, live, assignment)` → "match"|"mismatch"|
  "unknown",给分配区小徽标(对齐 PresetList 的 tri-state 语义)。

### 4.2 LiveProviderList.tsx(新组件)

行式列表,样式对齐 PresetList 卡片语言(`.ml-live-*` 新类,徽标复用
`.ml-badge*`):

- 折叠态:name(fallback id)+ 协议徽标(api 缺失不显示)+ baseUrl + 模型数 +
  **当前默认徽标**(pi:`live.provider === id`;opencode:`model` 前缀 `id/`)+
  操作[同步到供应商库][编辑][删除]+ 展开箭头;
- 展开:模型 id chip 列表;
- 编辑:展开态内联表单(name/baseUrl/api/apiKey,apiKey 留空=不改,占位符
  提示「留空保持不变」;显式清除输入 `<clear>`?——MVP:留空=不变,不支持
  从 UI 清 key,清 key 走同步到库后统一管理)→ 保存调 PUT;
- 删除:confirm(对齐 PresetList 删除交互)→ DELETE;
- 同步:POST sync(id)→ 提示「已导入 n / 跳过 n」+ fetchConfig 刷新库。

### 4.3 AgentTabs.tsx 改造

- 保留:安装徽标、供应商 select(compat 过滤 + current 可选规则)、保存/应用
  条、结果面板、未安装 amber 横幅。
- 单行 readback 替换为:分配区头部 live 匹配小徽标 + 下方「现有配置」区
  (`<LiveProviderList>`,live 为 null 或 providers 空 → 空态文案;未安装 →
  「未安装——预写模式」提示,live 区整块隐藏)。
- **模型选择接 ModelPicker**:select 换成「触发按钮(当前模型名/id)+ 展开
  ModelPicker 搜索列表」,props 直连 `config.providers[currentProviderId].models`;
  供应商未选 → 禁用;models 空 → 空态提示(引导去供应商页维护模型)。
  不再提供手填(validate_assignment 本就强制 model ∈ provider.models)。

### 4.4 ModelsPane.tsx

新增 handlers:`syncLiveProvider(agent, id?)`、`deleteLiveProvider(agent, id)`、
`editLiveProvider(agent, id, patch)`;edit/delete 结果写进现有 applyResult
状态(复用结果面板),sync 结果走 agentSaveMsg 式提示;操作后 fetchAgents +
fetchConfig 刷新。i18n 双语新增约 12 键(maLive* / maSync* / maPickModel 等)。

## 5. 测试

- `store.rs`:import_from_opencode —— 正常映射(baseURL/apiKey/headers/models
  name)/ anthropic npm 反推 / baseURL 缺失入 skipped / skip-existing 幂等 /
  文件缺失 NotFound / 损坏 Corrupt / **json5 注释 fixture**。
- `render/pi.rs`:edit 字段合并保留兄弟 provider + 节点自身 models + 未知顶层
  键;delete 删键 + defaultProvider 命中时清 settings 两键、未命中不动 settings。
- `render/opencode.rs`:json5 注释文件 edit/delete 读不炸、其他键保留、delete
  前缀命中清顶层 model。
- `mod.rs`(首个 tests mod):read_live —— pi 正常 / models 缺失 providers=[] /
  models 损坏默认值仍在 / 双缺 live null;opencode 正常 / json5 注释 / 损坏
  null / 缺失 null。

## 6. 取舍与不做

- **live 编辑只到 provider 级字段**(name/baseUrl/api/apiKey):逐模型编辑走
  「同步到供应商库 → ProviderEditor 管理 → 应用写回」,避免复刻一套 live 模型
  编辑器。PRD「编辑 provider 节点」按此口径。
- **opencode 注释不保留**:沿用 apply 的 pretty-JSON 写通道(PRD Notes 指定
  不新开路径);「无损」= 键级无损 + json5 容错读。
- **不做**「全部同步」按钮(单行同步已覆盖;全量走顶部既有 import 入口,若
  UI 有;没有也不加,避免入口重复)。
- 不动 claude/codex preset 页、不动 canonical schema、不加新依赖。

## 7. 回滚

纯新增路由 + live Value 扩充(旧前端忽略新字段,不破坏)+ 前端组件级改造。
回滚 = revert 单 commit;无数据迁移,canonical 与原生文件格式零变化。
