# S2 设计：agent 指派三层结构（D4）

## 0. 现状事实（勘察结论，设计基准）

三层在数据上**已经存在**，本任务不引入新实体，只补「agent 子集」维度 + 卡片化 UI：

| 层 | 载体 | 位置 |
|---|---|---|
| L1 全局供应商库 | `CanonicalConfig.providers` | mgr kv `models_profiles` 内每个 Profile 持有一整份 CanonicalConfig（mgr/src/models.rs:80-98）；aio-models store.rs:22-30 |
| L2 profile × agent 指派 | `AgentsConfig`（pi/opencode=`AgentAssignment{provider,model}`；claude/codex=`{presets,current}`） | aio-models store.rs:46-118/178-212，per-agent 字段本就独立 |
| L3 沙箱指派 | `assignments: BTreeMap<sandbox, profile_id>` | mgr kv `models_profiles`（models.rs:91-98）；PUT /api/sandboxes/:name/model_profile（routes.rs:1087-1110） |

同步链：mgr `GET /api/models/sync?name=` 返回 `{version, config}`（UNMASKED，models.rs:595-625）→ app mgr_sync 60s 拉取（mgr_sync.rs:72-97）→ `overwrite_and_render` 写本地 `~/.aio/models.json` + `apply_all_agents` 渲染（mgr_sync.rs:204-222）→ 渲染器四件套写原生配置文件。

**渲染只发生在沙箱 app 端**；mgr 不渲染任何 agent 文件。渲染器复用是免费获得的（R4 天然满足，唯一改动是加 agent 过滤参数）。

## 1. 数据模型（mgr/src/models.rs）

### 1.1 StoredAssignment

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StoredAssignment {
    pub profile: Option<String>,            // None = 解除指派（等价旧 null）
    #[serde(default)]
    pub agents: Option<Vec<String>>,        // None = 全部四 agent（旧数据语义）；Some(vec![]) = 零指派
}
```

- `StoredModels.assignments` 类型从 `BTreeMap<String, String>` 改为 `BTreeMap<String, StoredAssignment>`。
- **向后兼容（AC4）**：kv payload 反序列化加兼容层——旧形状（value 是裸 String）迁移为 `{profile: <s>, agents: None}`。实现：`read_assignments` 内先按新形状解析，失败则按旧形状解析并就地转换（不强制写回，下次 set_assignment 自然升级 version）。
- 现有函数适配：`assigned_profile` / `read_assignments` / `set_assignment`（签名增 `agents: Option<Vec<String>>` 参数或改收 `StoredAssignment`）；`list_sandboxes`/`sandbox_json` 输出增 `"model_agents": Option<Vec<String>>`（None 序列化为 null，前端 null=全指派）。

### 1.2 语义表（唯一权威）

| agents 值 | 含义 | sync 端点行为 |
|---|---|---|
| `None` / 旧数据 | 全指派 | 正常返回 payload，`agents: null` |
| `Some([pi, claude])` | 子集 | 正常返回 payload，`agents: ["pi","claude"]` |
| `Some([])` | 零指派 | **返回 404**（与未指派同路径 → app 端保留本地，AC3） |
| profile=None | 解除指派 | 404（现状不变） |

## 2. API（mgr/src/routes.rs）

### 2.1 PUT /api/sandboxes/:name/model_profile

```rust
struct ModelProfileBody {
    profile: Option<String>,          // 语义不变
    #[serde(default)]
    agents: Option<Vec<String>>,      // 缺省 = 全指派（旧客户端兼容）
}
```

- handler 校验：agent 名必须 ∈ {pi, claude, codex, opencode}（白名单，非法 400）；profile id 校验沿用 `set_assignment` 的 404。
- 响应增 `"model_agents"` 字段。
- **不触发 recreate、不进 envhash**（模型指派是运行时同步，与 S1 服务开关正交）。

### 2.2 GET /api/models/sync?name=

`SyncPayload` 增字段：`{version, config, agents: Option<Vec<String>>}`。
- `agents: null` = 全指派（app 端旧语义兜底）。
- 零指派/未指派 → 404（见 §1.2）。
- 同步更新 `GET /api/models/profiles` 的 `ModelProfile`？不需要——`assigned` 计数不变。

## 3. app 端（mgr_sync + 渲染）

### 3.1 SyncPayload（app/src/mgr_sync.rs:55-60）

```rust
pub struct SyncPayload {
    pub version: u64,
    pub config: CanonicalConfig,
    #[serde(default)]
    pub agents: Option<Vec<String>>,   // None/缺省 = 全部
}
```

### 3.2 apply 逻辑（mgr_sync.rs:102-127, 189-222）

- 差异判定从「只比 config」扩展为「比 config + agents 集合」（agents 变了也要重渲染；实现：`config_differs` 增参或在 apply 处合并比较）。
- `overwrite_and_render` 增 `agents: Option<&[String]>` 参数，透传给渲染过滤。
- app/src/routes/models/mod.rs 的 `apply_all_agents`(312-327) 增过滤变体：`apply_selected_agents(home, config, agents: Option<&[Agent]>)`——`None` 走现有全量路径；`Some` 只渲染白名单内、且有 assignment 的 agent。**不改四个 render/*.rs**（R4：零格式适配）。
- Agent 白名单解析复用 common.rs `Agent::from_str`(346)，未知名忽略并 warn。

### 3.3 不做的事

- 被移出子集的 agent **不清理**已写出的本地配置文件（PRD R3 明确「未指派 agent 的本地配置不动」）。
- 本地 `~/.aio/models.json` 仍整体覆盖（canonical store 是整份的，过滤只作用于渲染层）。

## 4. mgr-web 前端

### 4.1 R1：Models 页 agent 卡片化

范围：ModelsPage 五 tab 中的四个 agent tab（providers tab 不动）。

- **pi / opencode tab**：现 `AgentTabs`（provider 下拉 + ModelPicker）上方增「供应商卡片墙」——复用 ProviderGrid 的卡片视觉（新组件 `AgentProviderCards` 或给 ProviderGrid 加 `pickMode` prop），每卡显示 provider name/base_url/模型数，**当前 assignment 指向的卡高亮 ACTIVE 态**；点卡片 = `setAssignment({provider, model})`，model 取该 provider 首个模型（若 provider 未变则保留现 model）。下方保留 ModelPicker 供改具体 model。
- **claude / codex tab**：卡片 = preset 卡（preset 本就是「指向供应商库项」的引用，不复制 key）。`PresetList` 从列表改为卡片网格，**current preset 卡高亮**；点卡片 = setCurrent + save（现有一键语义，PresetList.tsx:9-10）。preset 的增删编辑表单保留。
- 数据流不变：仍走现有 `putModelsConfig(?profile=)` 编辑 AgentsConfig，沙箱端 60s 拉取生效（无 Apply 按钮，与 AgentTabs.tsx:136 注释一致的既有心智）。

### 4.2 R2：沙箱指派 UI 含 agent 勾选

- **共享组件** `AgentAssignControl`（新文件 `mgr-web/src/components/AgentAssignControl.tsx`）：四 agent 复选（pi/claude/codex/opencode 固定顺序），`value: string[] | null`（null=全选显示「全部 agent」），变更回调整个子集。
- **EditPage.tsx**（242-257 处）：`mpAssignTo` select 下方挂 AgentAssignControl（选中 profile 时显示）；提交走现有 profile-only 分支（EditPage.tsx:149-157），`putSandboxModelProfile` body 增 `agents`（全选时发 null，零选时发 `[]`）。
- **SandboxListPage.tsx**：profile chip（286-288）升级为点击弹出的轻量 popover（profile select + AgentAssignControl），不进 EditPage 即可改指派；chip 文案 `P:<name>` 追加 agent 摘要（全指派不显示，子集显示 `pi+2`）。
- **api.ts**：`putSandboxModelProfile`(224-229) body 增 `agents?: string[] | null`；`types.ts:74-76` Sandbox 增 `model_agents: string[] | null`。
- **types/api 兼容**：后端旧响应无 `model_agents` → 前端 `?? null` 兜底（null=全指派，AC4 UI 不回归）。

## 5. 兼容与回滚

- 旧 app + 新 mgr：payload 多一个 `agents` 字段，app `#[serde(default)]` 忽略 → 全量渲染，不回归。
- 新 app + 旧 mgr：`agents: None` → 全量渲染，不回归。
- 旧 kv payload：兼容层迁移（§1.1）。
- 回滚 = revert 提交即可；kv 新形状 payload 需旧代码能读——**不行**，旧代码 `BTreeMap<String,String>` 读新形状会失败。回滚说明：revert 后需手工删 kv `models_profiles` 行或手工把 assignments value 改回字符串（写进 implement.md 回滚节）。

## 6. 测试锚点

- mgr models.rs 单测：旧 payload 迁移、agents 白名单校验、Some([]) sync 404。
- app mgr_sync 单测（如有现成 pattern 则跟随；无则以集成验收为准）。
- 手工验收：AC2/AC3 按 prd，`make mgr-up` 重建（注意 memory：compose up --build 不重建运行中服务，须显式 build + --force-recreate）。
