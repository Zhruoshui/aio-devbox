# Design - 供应商表单 pi-web 流 + models.dev 集成

> prd.md 见同目录。参考调研:`.trellis/tasks/archive/2026-08/08-26-unified-model-config/research/pi-web-model-config.md`
> §2B(models.dev)。canonical cost 单位已在 08-27-usage-correctness 定为 **$/M**(见
> `model-config-guide.md` Cost backfill 段)——本任务的 models.dev 归一化与 pi 渲染都要对齐
> 这个既定单位,不再按 prd.md 旧注(写于该决定之前)所说的 $/token 换算。

## 0. 单位口径修正(承接 R5 决定,推翻 prd.md 旧注)

- prd.md Notes 原文:「models.dev 价格单位是 $/M tokens,需换算成 $/token」—— 这是在
  R5(08-27-usage-correctness)把 canonical cost 定为 **$/M** 之前写的,已过时。
  **现在 canonical 就是 $/M,models.dev 原生也是 $/M,两边一致,不需要任何换算。**
  `flattenModelsDevCatalog` 归一化时直接原样取用 models.dev 的 `cost.{input,output,
  cache_read,cache_write}`(单位 $/M),写入 `CostEntry` 即可。
- **发现的既有 bug(顺带修,属本任务改动面)**:`render/pi.rs::render_pi_cost` 把
  canonical `CostEntry`(现在是 $/M)原样透传进 pi 的 `models.json`,但 pi 原生 schema
  的 cost 字段是 **USD/token**(调研文档 pi-web-model-config.md §1:「cost 单位:
  USD/token」)。这会导致 apply 到 pi 之后,pi 自己算的 cost 偏差 100 万倍。
  **修复**:`render_pi_cost` 在写出前除以 1e6(`v / 1_000_000.0`)。这是本任务改动
  `ModelTable`/models.dev 填充 cost 字段时顺带暴露的正确性问题,一并修,不新开子任务
  (影响面小、就在改动路径上)。

## 1. 后端 `GET /api/models/catalog`

### 1.1 归一化目标结构

```rust
#[derive(Serialize, Clone)]
pub struct CatalogModel {
    pub id: String,               // models.dev model key
    pub name: Option<String>,
    pub reasoning: Option<bool>,
    pub input: Option<Vec<String>>,       // modalities, if present
    pub context_window: Option<u64>,
    pub max_tokens: Option<u64>,
    pub cost: Option<CostEntry>,          // reuse store.rs CostEntry, $/M
}

#[derive(Serialize, Clone)]
pub struct CatalogProvider {
    pub id: String,                // models.dev provider key
    pub name: String,
    pub base_url_hint: Option<String>,    // for host-based recommend matching
    pub models: Vec<CatalogModel>,
}

#[derive(Serialize)]
pub struct CatalogResponse {
    pub providers: Vec<CatalogProvider>,
    pub fetched_at: String,       // ISO — lets frontend show staleness if wanted (not required by AC)
}
```

- 解析 `https://models.dev/api.json` 的原始形状为 `Value`(容错,字段名/嵌套可能随源站变;
  只挑需要的字段,缺失就是 `None`,不报错、不 panic)。
- **推荐匹配**(“按 provider baseUrl 主机名/id”):归一化阶段不做匹配决策——匹配逻辑
  下放到前端(前端已知当前编辑的 provider 的 `baseUrl`/`api`,做 `hostname` 包含式匹配
  更简单,后端不需要猜)。后端只提供扁平化目录,`recommendModelCatalogPreset` 式的推荐
  在前端一个纯函数里做(`catalogRecommend(providers, baseUrl, modelId)`)。

### 1.2 缓存 + in-flight 去重(仿 usage.rs 的 `OnceLock<Mutex<...>>`)

```rust
struct CatalogCache { fetched_at_instant: Instant, data: Arc<CatalogResponse> }
static CATALOG_CACHE: OnceLock<Mutex<Option<CatalogCache>>> = OnceLock::new();
const CATALOG_TTL: Duration = Duration::from_secs(3600); // 1h
```

- 用 `tokio::sync::Mutex` 本身做 in-flight 去重:handler 拿锁后先查缓存命中即返回;
  未命中则**在持锁状态下**发起 fetch + 写缓存 + 释放锁 —— 并发请求天然排队,第二个
  请求拿到锁时缓存已被第一个填好,直接命中,不需要额外的 broadcast/oneshot 机制
  (与 usage.rs 的锁粒度做法一致,足够本场景:目录接口低频调用)。
- 15s 超时(`reqwest` client 已挂在 `AppState`,同 discover.rs 复用同一个 `Client`)。
- 失败:502 + 上游 body 截断 500 字符(与 discover/test 一致的错误契约)。
- 无 `?refresh=1` 参数(models.dev 目录变化慢,1h TTL 够用,不像 usage 需要强制刷新)。

### 1.3 路由注册

`app/src/main.rs`:`.route("/api/models/catalog", get(get_catalog))`,
`mod.rs` 或新文件 `app/src/routes/models/catalog.rs` 仿 `discover.rs` 布局。

## 2. 前端:ModelPicker 可复用组件(R2/R3 前置)

```ts
// web/src/panes/models/ModelPicker.tsx
export function ModelPicker({
  models,          // ModelEntry[] — target provider's current models
  onPick,          // (modelId: string) => void
}): JSX.Element
```

- **本任务只需要 provider 编辑器内部消费它的一个子场景**(从已加入的模型里挑一个做
  models.dev 填充目标);R2/R3 复用同一个组件做「选供应商 + 选模型」的双级选择器,
  那层是 R2/R3 自己的 wrapper,不在本任务实现——本任务把组件做成**无状态、纯 props**
  就够两边复用,不引入 R2/R3 才需要的 provider 维度。

## 3. 抽屉内部重构(`ProviderEditor.tsx`,只改内部布局,不改外层卡片网格)

- 两区结构:
  - **供应商信息区**:现有 basic info + advanced 不变(名称/baseUrl/协议/apiKey/headers/compat)。
  - **模型接入区**:现有「拉取模型」discover 流程保留;「加入所选」后,模型列表从
    **横向大表**(`ModelTable`,13 列)改为**逐行可展开卡片**:
    - 折叠态一行:id + name + reasoning 徽标 + cost 摘要(如有)+ 展开箭头 + 删除。
    - 展开态:name / reasoning / contextWindow / maxTokens / cost 四项(input/output/
      cacheRead/cacheWrite)+「从 models.dev 填充」按钮 + 高级(input 类型、
      thinkingLevelMap、per-model 协议覆盖、headers)。
  - **组件拆分**:新增 `ModelRow.tsx`(单模型折叠/展开行,替代 `ModelTable.tsx` 的
    表格渲染;`ModelTable.tsx` 整体废弃,`ProviderEditor` 改为渲染
    `provider.models.map(m => <ModelRow .../>)`)。
  - Test 按钮/pill 逻辑从 `ModelTable` 平移到 `ModelRow`(状态管理不变,仍是
    `ModelsPane` 拥有 `testState`,`ModelRow` 只是消费)。

## 4. models.dev 填充按钮

- `ModelRow` 展开态内「从 models.dev 填充」:
  1. 若 catalog 未拉取(懒加载,首次点击才 `GET /api/models/catalog`,之后前端内存
     缓存整份 catalog,同一次编辑会话内不重复请求)。
  2. 命中规则(前端 `catalogRecommend`):优先 `provider.baseUrl` 的 hostname 与
     catalog provider 的已知域名映射匹配(硬编码常见厂商域名表,仿 pi-web 的
     `@lobehub/icons` 硬编码映射思路,但这里映射的是 host→models.dev provider id);
     其次按当前 `provider.api` 协议做粗过滤;在匹配到的 provider 下按 `model.id`
     精确匹配(大小写不敏感)找目标 model。
  3. 命中:一次性 patch `name/reasoning/contextWindow/maxTokens/cost`(6 项,cost
     四个子项算 1 组);不覆盖用户已手填的非空字段?**不做增量保留** —— pi-web 原始
     行为是整块覆盖(一键填充语义就是"用 models.dev 的权威值替换"),按此实现,
     简单且符合"一键填充"预期。
  4. 未命中/请求失败:按钮置灰 + `title` 提示("未在 models.dev 目录中找到匹配项"/
     "无法访问 models.dev"),不阻断其他编辑,不抛错弹窗。

## 5. 不做

- 不改供应商库页签的整体布局(卡片网格 + 右侧抽屉架构不变,prd.md 已定)。
- 不做 OAuth/托管凭据(pi-web 的 auth.json 概念不适用——本项目 canonical apiKey 内联
  存储已是既定设计,R1 不改)。
- 不做 provider 图标映射(pi-web 的 `@lobehub/icons` 硬编码,超出本任务 Kumo 视觉范围)。
- 不在 R1 做 R2/R3 的「供应商+模型」双级选择器 wrapper——只做 ModelPicker 底层组件。
- catalog 接口不做 `?refresh=1`(1h TTL 足够,models.dev 目录变化慢)。

## 6. 测试策略

- 后端:catalog 归一化(mock/fixture JSON → `CatalogResponse` 字段映射正确);
  缓存命中(第二次调用不重复 fetch,断言只发一次网络请求);超时/非 2xx → 502 +
  截断 body;`render_pi_cost` 除以 1e6 的单测(输入 $/M 值,断言输出 pi 侧
  USD/token 值,如 canonical `input=0.14` → pi `cost.input=0.00000014`)。
- 前端:`catalogRecommend` 纯函数单测(host 匹配命中/大小写不敏感/未命中 →
  undefined);`npm run build` 干净;容器手测「新增供应商 → 拉取模型 → 加入 →
  展开某模型 → 填充 → 数值落到表单」全流程。
