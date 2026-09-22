# S2 执行计划

前置：design.md §1-§5 为实现唯一依据；改动顺序按数据流方向（mgr 存储 → mgr API → app 同步 → 前端），每步可独立编译。

## Step 1 mgr 数据模型（mgr/src/models.rs）

- [ ] `StoredAssignment` 结构 + `StoredModels.assignments` 换型（design §1.1）
- [ ] `read_assignments` 兼容层：旧 String payload → 新形状（AC4）
- [ ] `assigned_profile` / `set_assignment` 适配；agent 名白名单常量 `VALID_AGENTS = ["pi","claude","codex","opencode"]`
- [ ] 单测：旧 payload 迁移、未知 agent 拒绝、Some([]) 语义
- 验证：`cargo test -p aio-mgr`

## Step 2 mgr API（mgr/src/routes.rs + models.rs sync）

- [ ] `ModelProfileBody` 增 `agents: Option<Vec<String>>`；handler 校验白名单（非法 400），响应增 `model_agents`
- [ ] `sandbox_json` 输出 `model_agents: Option<Vec<String>>`
- [ ] `GET /api/models/sync` payload 增 `agents`；零指派/未指派 → 404（design §1.2）
- 验证：`cargo test -p aio-mgr`；`cargo build -p aio-mgr`

## Step 3 app 端同步与渲染过滤

- [ ] app `SyncPayload` 增 `#[serde(default)] agents: Option<Vec<String>>`（mgr_sync.rs:55-60）
- [ ] apply 差异判定扩展为 config + agents 集合（mgr_sync.rs:189）
- [ ] `overwrite_and_render` 增 agents 参数；models/mod.rs 增 `apply_selected_agents`（过滤 variant，不动 render/*.rs）
- [ ] 未知 agent 名忽略 + warn
- 验证：`cargo test -p aio-app`

## Step 4 前端类型与 API client

- [ ] `mgr-web/src/types.ts` Sandbox 增 `model_agents: string[] | null`
- [ ] `api.ts` `putSandboxModelProfile` body 增 `agents?: string[] | null`；profile 列表/详情类型不动
- 验证：`cd mgr-web && npx tsc --noEmit`

## Step 5 AgentAssignControl 组件 + EditPage

- [ ] 新建 `mgr-web/src/components/AgentAssignControl.tsx`（四 agent 复选，null=全选；遵循 frontend/component-guidelines）
- [ ] EditPage 挂载（profile 选中时显示），提交逻辑进现有 profile-only 分支
- 验证：tsc + 页面手工点检

## Step 6 SandboxListPage 快捷指派

- [ ] profile chip → popover（profile select + AgentAssignControl）
- [ ] chip 文案 agent 摘要（子集显示 `pi+2`，全指派不显示）
- 验证：tsc

## Step 7 Models 页 agent 卡片化（R1）

- [ ] pi/opencode tab：供应商卡片墙（active 高亮，点卡=setAssignment），保留 ModelPicker
- [ ] claude/codex tab：PresetList 卡片网格化（current 高亮，点卡=setCurrent+save），CRUD 保留
- 验证：tsc + `npm run build`

## Step 8 质量检查（2.2 全量）

- [ ] `cargo test -p aio-mgr -p aio-app` 全绿
- [ ] `cd mgr-web && npx tsc --noEmit && npm run build` 全绿
- [ ] 手工链路验收（AC1-AC4）：make mgr-up 重建（显式 build + --force-recreate，见 memory no-cache/compose 坑）→
      沙箱 A 指派 profile P + 仅 pi/opencode → 60s 内 ~/.pi 更新、~/.claude ~/.codex 不变 →
      agent 全不勾 → 拉取后本地配置完全不动 → 旧沙箱无 agents 字段行为不变
- [ ] dispatch trellis-check 做规范复查

## Step 9 收尾

- [ ] 3.3 spec 更新：model-config-guide.md（agents 子集语义）+ api-contracts.md（model_profile body/sync payload 变更）
- [ ] 3.4 提交（feat(mgr): S2 …）+ journal 记录

## 回滚点

- 每个 Step 独立可编译，revert 按 commit 粒度。
- kv `models_profiles` 新形状 payload 与旧代码不兼容（design §5）：若需回滚到旧代码，先
  `sqlite3` 删 kv 行 `models_profiles` 或手工把 assignments value 改回字符串——此操作会丢
  agents 子集信息（回到全指派），profile/供应商数据不受损。
