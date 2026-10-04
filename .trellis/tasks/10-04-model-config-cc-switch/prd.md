# 模型配置页重构为 cc-switch 模式

## Goal

将模型配置从当前的"mgr 统一管理 + profile 分配"模式，重构为**纯 sandbox 内部配置**模式。每个 sandbox 的四大 agent（pi/opencode/claude/codex）完全独立配置，互不影响。供应商库保留在 mgr 层作为共享模板源。

## Background

当前模型配置采用 mgr 统一管理：
- mgr 维护 CanonicalConfig（供应商库 + agent 分配）
- 支持多 profile，每个 sandbox 选择应用哪个 profile
- 复杂的 sandbox 级 agent 子集管理

用户希望简化为：
- **每个 sandbox 独立配置自己的 agent**
- **各 sandbox 之间完全独立，互不影响**
- **配置时可以从 mgr 的供应商库下拉选择模板**
- **严格遵循 cc-switch 的每 agent 配置模式**

## Architecture Changes

### 废弃的功能
- ❌ mgr 层的 profile 系统（ModelProfile）
- ❌ mgr 层的统一 agent 开关（per-sandbox agent 子集）
- ❌ mgr 层的 sandbox 级 agent 管理（SandboxTable）
- ❌ "从 pi 导入"功能
- ❌ models.dev 目录联动
- ❌ 供应商库的"绑定的 Agent"展示

### 保留的功能
- ✅ mgr 层的供应商库（作为共享模板源）
- ✅ sandbox 内部的 agent 配置（cc-switch 模式）
- ✅ Usage 统计（简化为从 sandbox 原生配置读取）

### 新的数据流
```
┌─────────────────┐         ┌──────────────────┐
│   mgr 供应商库   │────────→│  sandbox A       │
│  (模板源)       │  下拉选择 │  ├─ claude 配置  │
└─────────────────┘         │  ├─ codex 配置   │
                            │  ├─ pi 配置      │
                            │  └─ opencode 配置│
                            └──────────────────┘
                            ┌──────────────────┐
                            │  sandbox B       │
                            │  ├─ claude 配置  │
                            │  ├─ codex 配置   │
                            │  ├─ pi 配置      │
                            │  └─ opencode 配置│
                            └──────────────────┘
```

## Requirements

### R1: 供应商库（mgr 层）
- 保留为独立标签页，角色转变为"模板仓库"
- 删除"从 pi 导入"按钮和相关 API
- 删除 models.dev 目录联动（catalog 相关 UI 和 API）
- 新增/编辑供应商改为中心弹窗（符合项目现有 Dialog 模式）
- 删除"被此供应商绑定的 Agent"展示

### R2: sandbox 内部 agent 配置（cc-switch 模式）
- **claude/codex（切换式）**：每个 provider 是完整配置快照
  - 图形化表单编辑（provider/model/apiKey/env 等）
  - 查看/编辑配置文件的源文件内容（JSON/TOML 原文）
  - 一键切换（current 标记）
- **pi/opencode（增量式）**：单文件多节点
  - 所有 provider 共存于原生配置文件
  - 图形化编辑 + 源文件内容查看
  - 切换 = 修改 default provider/model 指向

### R3: 供应商库与 sandbox 配置联动
- sandbox 配置 agent 时，可从 mgr 供应商库下拉选择
- 选择后自动填充 baseUrl/apiKey/models 等信息（模板复制）
- 拷贝后完全独立编辑，与供应商库不再关联

### R4: 简化 Usage 统计
- 不再依赖 mgr 统一追踪
- 直接从各 sandbox 的原生配置文件读取实际使用的 provider/model

## Technical Design

### 存储结构
- **mgr 层**：`models.json` 只存储供应商库（providers 字典）
- **sandbox 层**：每个 sandbox 的 app 服务存储自己的 agent 配置
  - `~/.claude/settings.json` + cc-switch 快照
  - `~/.codex/config.toml` + auth.json
  - `~/.pi/agent/models.json` + settings.json
  - `~/.config/opencode/opencode.jsonc`

### 后端 API
- **mgr 层**：
  - `GET/PUT /api/models/providers` — 供应商库 CRUD
  - `GET /api/models/templates` — 供 sandbox 下拉选择的模板列表
- **sandbox 层**（app 服务已有，需要重构）：
  - `GET/PUT /api/models/config` — 该 sandbox 的完整 agent 配置
  - `POST /api/models/apply/:agent` — 渲染到 agent 原生文件

### 前端页面
- **mgr 顶层导航**：保留"模型供应商"入口（供应商库页）
  - 简化版，只有 providers 标签页
  - 新增/编辑供应商使用中心弹窗
- **sandbox 详情页**：新增"模型配置"标签页
  - 包含四个 agent 标签页（claude/codex/pi/opencode）
  - 每个 agent 页：provider 列表 + 图形化编辑器 + 源文件查看器
  - 新增 provider 时可选择"从 mgr 供应商库导入"

## Decisions

- **D1**: 完全废弃 mgr 层的统一管理（profile、agent 开关、sandbox 子集），改为纯 sandbox 内部配置
- **D2**: 供应商库保留在 mgr 层作为共享模板源，sandbox 配置时可下拉选择
- **D3**: 采用 cc-switch 模式：claude/codex 切换式快照，pi/opencode 增量式单文件多节点
- **D4**: 源文件编辑器使用轻量方案（react-simple-code-editor + prismjs），支持 JSON/TOML 高亮
- **D5**: 不迁移现有数据，直接重新开始
- **D6**: 供应商库新增/编辑使用中心弹窗，删除"从 pi 导入"和 models.dev 联动

## Acceptance Criteria

- [ ] mgr 供应商库页：无"从 pi 导入"按钮，无 models.dev 联动，新增/编辑使用中心弹窗
- [ ] sandbox claude 配置页：可创建/编辑/删除/切换多个 provider 快照，可查看源文件
- [ ] sandbox codex 配置页：同上
- [ ] sandbox pi 配置页：可管理 providers 列表，可查看/编辑源文件，可切换默认
- [ ] sandbox opencode 配置页：同上
- [ ] 所有 sandbox agent 配置可从 mgr 供应商库下拉选择模板
- [ ] 各 sandbox 配置完全独立，互不影响
- [ ] Usage 统计简化为从 sandbox 原生配置读取

## Decisions (增补)

- **D7**: **保留跨 sandbox 的 Usage 统计汇总视图** —— mgr 层需要能看到所有 sandbox 的 Usage 总览，即使各 sandbox 配置独立。实现方式：mgr 定期从各 sandbox 的 app 服务拉取 Usage 数据并缓存，或提供实时查询接口。

## Dependencies

- **阻塞依赖**：当前 in_progress 任务 `image-detail-tools-log` 必须完成后才能开始本任务
- **分支策略**：在干净的工作区上新建 feature 分支进行开发
