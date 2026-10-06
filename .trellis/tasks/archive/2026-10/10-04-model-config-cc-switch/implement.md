# 模型配置页重构 - 实施计划

## 前置条件

- [ ] 当前 in_progress 任务 `image-detail-tools-log` 已完成并提交
- [ ] 新建 feature 分支 `feat/model-config-cc-switch`
- [ ] 工作区干净，无未提交改动

## 实施阶段

### Phase 1: 数据模型与后端基础（2-3天）

#### 1.1 定义新的数据模型
**文件**:
- `aio-models/src/store.rs` — 重命名/重构现有 CanonicalConfig
- `aio-models/src/template.rs` — 新增 ProviderTemplate 定义

**任务**:
- [ ] 定义 `ProviderTemplate` 结构体（mgr 层供应商库条目）
- [ ] 定义 `ProviderSnapshot` 结构体（sandbox 层切换式快照）
- [ ] 定义 `IncrementalAgentConfig` 结构体（sandbox 层增量式配置）
- [ ] 实现序列化/反序列化
- [ ] 编写单元测试

**验证**: `cargo test -p aio-models`

#### 1.2 重构 mgr 供应商库 API
**文件**:
- `mgr/src/models.rs` — 删除 profile 相关代码，保留供应商库 CRUD
- `mgr/src/routes/models.rs` — 简化路由

**任务**:
- [ ] 删除 `ModelProfile` 相关代码
- [ ] 删除 `GET/POST /api/models/profiles` 路由
- [ ] 删除 `PUT /api/sandboxes/:name/model_profile` 路由
- [ ] 实现 `GET/PUT /api/models/providers`（供应商库 CRUD）
- [ ] 实现 `GET /api/models/templates`（模板列表，apiKey masking）
- [ ] 更新测试

**验证**: `cargo test -p mgr`

#### 1.3 实现 sandbox 配置存储
**文件**:
- `app/src/models/config.rs` — 新增 sandbox agent 配置存储
- `app/src/models/snapshots.rs` — 新增切换式快照管理

**任务**:
- [ ] 实现 `SandboxAgentConfig` 存储（单文件 `~/.aio/models/config.json`）
- [ ] 实现 `SwitchAgentConfig` 的快照读写
- [ ] 实现 `IncrementalAgentConfig` 的节点读写
- [ ] 实现从 mgr 拉取供应商库模板并缓存
- [ ] 编写单元测试

**验证**: `cargo test -p app`

### Phase 2: 后端渲染（2-3天）

#### 2.1 实现 claude 切换式渲染
**文件**:
- `app/src/routes/models/render/claude.rs` — 重构为 cc-switch 模式

**任务**:
- [ ] 实现 `apply_claude_snapshot(snapshot)` — 整体替换 ~/.claude/settings.json
- [ ] 实现从快照中提取并渲染 env/permissions/hooks
- [ ] 实现快照的 backfill（切换前保存当前配置到旧快照）
- [ ] 编写集成测试

**验证**: `cargo test -p app claude`

#### 2.2 实现 codex 切换式渲染
**文件**:
- `app/src/routes/models/render/codex.rs` — 重构为 cc-switch 模式

**任务**:
- [ ] 实现 `apply_codex_snapshot(snapshot)` — 替换 config.toml + auth.json
- [ ] 实现双文件事务（失败回滚）
- [ ] 实现 TOML 模板解析和验证
- [ ] 编写集成测试

**验证**: `cargo test -p app codex`

#### 2.3 实现 pi/opencode 增量式渲染
**文件**:
- `app/src/routes/models/render/pi.rs` — 重构为增量式
- `app/src/routes/models/render/opencode.rs` — 重构为增量式

**任务**:
- [ ] 实现 `apply_pi_config(config)` — 读-改-合并 models.json
- [ ] 实现 `apply_opencode_config(config)` — 读-改-合并 opencode.jsonc
- [ ] 实现 default provider/model 切换
- [ ] 实现 JSON5 容错读取
- [ ] 编写集成测试

**验证**: `cargo test -p app pi opencode`

#### 2.4 重构 sandbox API
**文件**:
- `app/src/routes/models/mod.rs` — 简化路由

**任务**:
- [ ] 删除 `POST /api/models/import/pi` 路由
- [ ] 删除 `GET /api/models/catalog` 路由
- [ ] 删除 `PUT/DELETE /api/models/agents/:agent/provider/:id` 路由
- [ ] 实现 `GET/PUT /api/models/config`（新结构）
- [ ] 实现 `POST /api/models/apply/:agent`
- [ ] 实现 `GET /api/models/usage`
- [ ] 更新测试

**验证**: `cargo test -p app`

### Phase 3: mgr 供应商库前端（2-3天）

#### 3.1 重构 ModelsPage → ModelProvidersPage
**文件**:
- `mgr-web/src/pages/models/ModelsPage.tsx` — 重命名/重构
- `mgr-web/src/pages/models/types.ts` — 更新类型定义

**任务**:
- [ ] 重命名组件为 `ModelProvidersPage`
- [ ] 删除 profile 相关状态（profiles, profileId 等）
- [ ] 删除 SandboxTable 引用
- [ ] 简化 state 管理（只保留供应商库）
- [ ] 更新路由配置

**验证**: `cd mgr-web && npm run build`

#### 3.2 实现 ProviderEditorModal
**文件**:
- `mgr-web/src/components/ProviderEditorModal.tsx` — 新增
- `mgr-web/src/pages/models/ProviderEditor.tsx` — 删除或重构

**任务**:
- [ ] 创建中心弹窗组件（基于现有 Dialog 组件）
- [ ] 实现表单：name, baseUrl, api, apiKey, models
- [ ] 实现高级选项：headers, compat
- [ ] 实现"拉取模型"按钮（调用 discover API）
- [ ] 实现保存/取消逻辑

**验证**: 手动测试新增/编辑供应商

#### 3.3 删除废弃功能
**文件**:
- `mgr-web/src/pages/models/` — 清理

**任务**:
- [ ] 删除 `importPiModels` API 调用
- [ ] 删除 `getModelsCatalog` API 调用
- [ ] 删除 "Bound Agents" 相关 UI
- [ ] 删除 `AgentTabs.tsx`（sandbox 配置页实现后再删）
- [ ] 删除 `PresetList.tsx`
- [ ] 删除 `SandboxTable.tsx`
- [ ] 更新 i18n 字符串

**验证**: `cd mgr-web && npm run build`（无 TS 错误）

### Phase 4: sandbox 配置前端（4-5天）

#### 4.1 新增 sandbox 详情页"模型配置"标签
**文件**:
- `mgr-web/src/pages/SandboxDetailPage.tsx` — 新增标签页
- `mgr-web/src/pages/models/SandboxModelConfigTab.tsx` — 新增组件

**任务**:
- [ ] 在 sandbox 详情页路由中添加"模型配置"标签
- [ ] 创建 `SandboxModelConfigTab` 组件
- [ ] 实现四个 agent 子标签（claude/codex/pi/opencode）
- [ ] 实现从 sandbox app API 加载配置

**验证**: 手动测试 sandbox 详情页导航

#### 4.2 实现 SwitchAgentPanel（claude/codex）
**文件**:
- `mgr-web/src/pages/models/SwitchAgentPanel.tsx` — 新增

**任务**:
- [ ] 实现 provider 列表展示（卡片式）
- [ ] 实现 current 标记和一键切换
- [ ] 实现 ProviderEditor 集成
- [ ] 实现 SourceCodeEditor 集成
- [ ] 实现 TemplatePickerModal 集成

**验证**: 手动测试 claude 配置流程

#### 4.3 实现 IncrementalAgentPanel（pi/opencode）
**文件**:
- `mgr-web/src/pages/models/IncrementalAgentPanel.tsx` — 新增

**任务**:
- [ ] 实现 provider 节点列表
- [ ] 实现 default provider/model 选择器
- [ ] 实现 ProviderEditor（复用 SwitchAgentPanel 的编辑器）
- [ ] 实现 SourceCodeViewer（查看整个文件）
- [ ] 实现 TemplatePickerModal 集成

**验证**: 手动测试 pi/opencode 配置流程

#### 4.4 实现 SourceCodeEditor 组件
**文件**:
- `mgr-web/src/components/SourceCodeEditor.tsx` — 新增

**任务**:
- [ ] 集成 react-simple-code-editor
- [ ] 集成 prismjs（JSON/TOML 语法）
- [ ] 实现双向绑定（表单 ↔ 源代码）
- [ ] 实现解析失败提示
- [ ] 实现保存/取消逻辑

**验证**: 手动测试源代码编辑

#### 4.5 实现 TemplatePickerModal
**文件**:
- `mgr-web/src/components/TemplatePickerModal.tsx` — 新增

**任务**:
- [ ] 实现从 mgr API 拉取供应商库
- [ ] 实现模板列表展示
- [ ] 实现选择和确认逻辑
- [ ] 实现模板数据填充到表单

**验证**: 手动测试模板选择流程

### Phase 5: Usage 汇总（1-2天）

#### 5.1 实现 mgr Usage 聚合服务
**文件**:
- `mgr/src/usage.rs` — 重构

**任务**:
- [ ] 实现定期轮询各 sandbox 的 `/api/models/usage`
- [ ] 实现结果缓存（`mgr-data/usage_cache.json`）
- [ ] 实现 `GET /api/usage/summary` 路由
- [ ] 实现单个 sandbox usage 查询

**验证**: `cargo test -p mgr`

#### 5.2 实现 Usage 汇总页面
**文件**:
- `mgr-web/src/pages/UsagePage.tsx` — 重构

**任务**:
- [ ] 实现跨 sandbox 汇总视图
- [ ] 实现图表展示（复用现有 charts.tsx）
- [ ] 实现按 sandbox/agent/model 过滤
- [ ] 实现时间范围选择

**验证**: 手动测试 Usage 页面

### Phase 6: 清理与文档（1-2天）

#### 6.1 删除废弃代码
**文件**:
- `mgr/src/` — 删除 profile 相关代码
- `mgr-web/src/pages/models/` — 删除废弃组件

**任务**:
- [ ] 删除 `mgr/src/models.rs` 中的 profile 代码
- [ ] 删除 `mgr/src/routes/models.rs` 中的 profile 路由
- [ ] 删除 `AgentTabs.tsx`
- [ ] 删除 `PresetList.tsx`
- [ ] 删除 `SandboxTable.tsx`
- [ ] 删除 `importPiModels` 和 `getModelsCatalog` API
- [ ] 清理未使用的类型定义

**验证**: `cargo check` 和 `npm run build` 无警告

#### 6.2 更新文档
**文件**:
- `docs/` — 更新 API 文档
- `.trellis/spec/backend/api-contracts.md` — 更新 API 契约
- `.trellis/spec/frontend/` — 更新前端规范

**任务**:
- [ ] 更新 API 文档（新的端点和数据结构）
- [ ] 更新前端组件文档
- [ ] 更新架构图
- [ ] 编写迁移指南（尽管不迁移数据）

**验证**: 文档审查

## 验证命令

```bash
# 后端测试
cargo test --workspace

# 前端构建
cd mgr-web && npm run build

# 代码检查
cargo clippy --workspace
cd mgr-web && npm run lint

# 手动测试
# 1. 启动 mgr 和 sandbox app
# 2. 访问 mgr-web 供应商库页
# 3. 新增/编辑供应商
# 4. 进入 sandbox 详情页，配置各 agent
# 5. 验证原生配置文件已更新
# 6. 查看 Usage 汇总
```

## 风险与回滚

### 风险点
1. **数据丢失**：不迁移数据，用户需要重新配置
   - 缓解：提前通知用户，提供备份旧配置的选项

2. **API 破坏**：前端和后端 API 结构改变
   - 缓解：使用 feature flag 或版本控制，逐步切换

3. **sandbox app 兼容性**：旧的 sandbox app 可能不兼容新的 mgr
   - 缓解：保持 mgr 向后兼容，或强制升级 sandbox app

### 回滚点
- **Phase 1 完成后**：可以回滚到旧的数据模型
- **Phase 2 完成后**：可以回滚到旧的渲染逻辑
- **Phase 3 完成后**：前端供应商库页可以独立回滚
- **Phase 4 完成后**：sandbox 配置页可以独立回滚
- **Phase 5 完成后**：Usage 汇总可以独立回滚

## 时间估算

| 阶段 | 任务数 | 预估时间 |
|------|--------|----------|
| Phase 1: 数据模型与后端基础 | 3 | 2-3 天 |
| Phase 2: 后端渲染 | 4 | 2-3 天 |
| Phase 3: mgr 供应商库前端 | 3 | 2-3 天 |
| Phase 4: sandbox 配置前端 | 5 | 4-5 天 |
| Phase 5: Usage 汇总 | 2 | 1-2 天 |
| Phase 6: 清理与文档 | 2 | 1-2 天 |
| **总计** | **19** | **12-18 天** |

## 后续任务

- [ ] 编写端到端测试（Playwright）
- [ ] 性能优化（Usage 汇总缓存策略）
- [ ] 用户反馈收集和迭代
- [ ] 文档完善（用户指南、API 文档）
