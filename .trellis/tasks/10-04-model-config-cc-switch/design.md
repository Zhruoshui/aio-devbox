# 模型配置页重构 - 技术设计

## 架构概览

### 当前架构（废弃）
```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   mgr-web       │────→│   mgr (Rust)    │────→│  CanonicalConfig │
│  ModelsPage     │     │  /api/models/*  │     │  (统一存储)      │
│  (5个标签页)    │     │  profile 系统   │     │  providers[]    │
└─────────────────┘     └─────────────────┘     │  agents.*       │
                                                └─────────────────┘
                                                         ↓
                                                ┌─────────────────┐
                                                │  sandbox app    │
                                                │  (渲染到原生文件)│
                                                └─────────────────┘
```

### 新架构（目标）
```
┌─────────────────────────────────────────────────────────────────┐
│                        mgr 层                                   │
│  ┌─────────────────┐        ┌─────────────────┐                │
│  │  供应商库页面    │        │  Usage 汇总页   │                │
│  │  (模板仓库)      │        │  (跨sandbox)    │                │
│  └─────────────────┘        └─────────────────┘                │
│           ↓                           ↑                         │
│  ┌─────────────────┐        ┌─────────────────┐                │
│  │ mgr API         │        │ Usage 聚合服务  │                │
│  │ /models/providers│        │ (轮询各sandbox) │                │
│  │ /models/templates│        └─────────────────┘                │
│  └─────────────────┘                                            │
└─────────────────────────────────────────────────────────────────┘
                              ↓ (HTTP)
┌─────────────────────────────────────────────────────────────────┐
│                     sandbox 层 (每个独立)                        │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              sandbox 详情页 → "模型配置"标签               │   │
│  │  ┌───────┐ ┌───────┐ ┌───────┐ ┌───────┐              │   │
│  │  │claude │ │ codex │ │  pi   │ │open-  │              │   │
│  │  │配置   │ │配置   │ │配置   │ │code   │              │   │
│  │  └───────┘ └───────┘ └───────┘ └───────┘              │   │
│  └─────────────────────────────────────────────────────────┘   │
│           ↓                                                    │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │           sandbox app /api/models/* (重构后)               │   │
│  │  - GET/PUT config (该sandbox的完整agent配置)              │   │
│  │  - POST apply/:agent (渲染到原生文件)                     │   │
│  │  - GET usage (该sandbox的Usage统计)                       │   │
│  └─────────────────────────────────────────────────────────┘   │
│           ↓                                                    │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              原生配置文件 (cc-switch 模式)                  │   │
│  │  ~/.claude/settings.json  (切换式快照)                    │   │
│  │  ~/.codex/config.toml     (切换式快照)                    │   │
│  │  ~/.pi/agent/models.json  (增量式多节点)                  │   │
│  │  ~/.config/opencode/opencode.jsonc (增量式多节点)         │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

## 数据模型

### mgr 层 - 供应商库（简化版 CanonicalConfig）
```rust
// mgr/src/models.rs
pub struct ProviderLibrary {
    pub version: u32,
    pub providers: IndexMap<String, ProviderTemplate>,
}

pub struct ProviderTemplate {
    pub name: String,
    pub base_url: String,
    pub api: String,  // "openai-completions" | "openai-responses" | "anthropic-messages"
    pub api_key: Option<String>,
    pub headers: HashMap<String, String>,
    pub models: Vec<ModelEntry>,  // 可选，支持动态拉取
    // cc-switch 风格元数据
    pub website_url: Option<String>,
    pub category: Option<String>,  // official | cn_official | aggregator | third_party | custom
    pub notes: Option<String>,
}

pub struct ModelEntry {
    pub id: String,
    pub name: Option<String>,
    pub reasoning: Option<bool>,
    pub context_window: Option<u64>,
    pub max_tokens: Option<u64>,
    pub cost: Option<CostEntry>,
}
```

### sandbox 层 - Agent 配置（cc-switch 模式）
```rust
// app/src/models/config.rs (新文件)
#[derive(Serialize, Deserialize)]
pub struct SandboxAgentConfig {
    pub claude: Option<SwitchAgentConfig>,   // 切换式
    pub codex: Option<SwitchAgentConfig>,    // 切换式
    pub pi: Option<IncrementalAgentConfig>,  // 增量式
    pub opencode: Option<IncrementalAgentConfig>, // 增量式
}

// claude/codex: 切换式 - 每个 provider 是完整快照
#[derive(Serialize, Deserialize)]
pub struct SwitchAgentConfig {
    pub providers: Vec<ProviderSnapshot>,
    pub current: Option<String>,  // 当前生效的 provider id
}

#[derive(Serialize, Deserialize)]
pub struct ProviderSnapshot {
    pub id: String,
    pub name: String,
    pub settings_config: serde_json::Value,  // cc-switch 风格的不透明配置
    pub created_at: Option<i64>,
    pub notes: Option<String>,
}

// pi/opencode: 增量式 - 单文件多节点
#[derive(Serialize, Deserialize)]
pub struct IncrementalAgentConfig {
    pub providers: IndexMap<String, ProviderNode>,
    pub default_provider: Option<String>,
    pub default_model: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ProviderNode {
    pub name: String,
    pub base_url: String,
    pub api: String,
    pub api_key: Option<String>,
    pub models: Vec<ModelEntry>,
    // pi/opencode 特有字段
    pub headers: Option<HashMap<String, String>>,
    pub compat: Option<serde_json::Value>,
}
```

## API 设计

### mgr 层 API
```rust
// 供应商库 CRUD
GET    /api/models/providers          // 获取整个供应商库
PUT    /api/models/providers          // 整体替换（前端编辑后保存）
POST   /api/models/providers          // 新增单个 provider
PUT    /api/models/providers/:id      // 更新单个
DELETE /api/models/providers/:id      // 删除单个

// 模板列表（供 sandbox 下拉选择）
GET    /api/models/templates          // 返回简化的模板列表 [{id, name, baseUrl, api, models}]

// Usage 汇总
GET    /api/usage/summary             // 跨 sandbox 汇总
GET    /api/usage/sandbox/:name       // 单个 sandbox 的 usage
```

### sandbox 层 API（重构 app/src/routes/models/）
```rust
// 配置管理
GET    /api/models/config             // 获取该 sandbox 的完整 agent 配置
PUT    /api/models/config             // 保存完整配置
POST   /api/models/apply/:agent       // 渲染到 agent 原生文件

// 供应商模板（从 mgr 拉取后缓存）
GET    /api/models/templates          // 缓存的 mgr 供应商库（只读）

// Usage
GET    /api/models/usage              // 该 sandbox 的 Usage 统计
```

## 前端组件设计

### mgr-web 供应商库页
```
ModelProvidersPage (顶层路由 /models/providers)
├── ProviderGrid (卡片网格)
│   ├── ProviderCard
│   │   ├── name, baseUrl, api, models 数量
│   │   └── 操作: 编辑 / 删除
│   └── AddProviderCard (+ 新增)
├── ProviderEditorModal (中心弹窗)
│   ├── 表单: name, baseUrl, api, apiKey, models
│   ├── 高级: headers, compat
│   └── 操作: 保存 / 取消
└── (删除: import from pi, models.dev, bound agents)
```

### sandbox 详情页 - 模型配置标签
```
SandboxModelConfigTab (新增组件)
├── AgentTabs (claude | codex | pi | opencode)
│   ├── SwitchAgentPanel (claude/codex)
│   │   ├── ProviderList
│   │   │   ├── ProviderCard (含 current 标记)
│   │   │   └── AddProviderButton
│   │   ├── ProviderEditor
│   │   │   ├── 图形化表单
│   │   │   └── SourceCodeEditor (JSON/TOML)
│   │   └── TemplatePickerModal (从 mgr 供应商库选择)
│   └── IncrementalAgentPanel (pi/opencode)
│       ├── ProviderList (多节点)
│       ├── DefaultSelector (default provider/model)
│       ├── ProviderEditor (同切换式)
│       └── SourceCodeViewer (整个文件的 JSON/JSONC)
└── UsageSummary (简化的 Usage 展示)
```

### 共享组件
```
SourceCodeEditor
├── react-simple-code-editor (轻量编辑器)
├── prismjs (JSON/TOML 语法高亮)
├── 双向绑定: 表单 ↔ 源代码
└── 解析失败提示（不阻塞编辑）

TemplatePickerModal
├── 从 mgr API 拉取供应商库
├── 列表展示: name, baseUrl, api, models
└── 选择后触发 onSelect(template) 回调
```

## 存储路径

### mgr 层
- 供应商库: `mgr-data/models.json` (简化版 CanonicalConfig)
- Usage 缓存: `mgr-data/usage_cache.json` (可选，用于汇总)

### sandbox 层
- claude 快照存储: `~/.aio/models/claude_snapshots.json` (切换式快照)
- codex 快照存储: `~/.aio/models/codex_snapshots.json`
- pi 配置: `~/.pi/agent/models.json` (原生)
- opencode 配置: `~/.config/opencode/opencode.jsonc` (原生)

## 渲染逻辑（cc-switch 模式）

### claude（切换式）
```rust
// 当 current provider 切换时
fn apply_claude(home: &Path, snapshot: &ProviderSnapshot) -> Result<()> {
    let settings: serde_json::Value = serde_json::from_value(snapshot.settings_config.clone())?;
    // settings 包含完整 ~/.claude/settings.json 内容
    let path = home.join(".claude/settings.json");
    atomic_write(&path, &serde_json::to_string_pretty(&settings)?)?;
    Ok(())
}
```

### codex（切换式）
```rust
fn apply_codex(home: &Path, snapshot: &ProviderSnapshot) -> Result<()> {
    let config: CodexConfig = serde_json::from_value(snapshot.settings_config.clone())?;
    // config.toml
    let toml_path = home.join(".codex/config.toml");
    atomic_write(&toml_path, &toml::to_string(&config.config)?)?;
    // auth.json
    if let Some(auth) = config.auth {
        let auth_path = home.join(".codex/auth.json");
        atomic_write(&auth_path, &serde_json::to_string_pretty(&auth)?)?;
    }
    Ok(())
}
```

### pi/opencode（增量式）
```rust
fn apply_pi(home: &Path, config: &IncrementalAgentConfig) -> Result<()> {
    let path = home.join(".pi/agent/models.json");
    // 读-改-合并：保留文件中其他键，只更新 providers
    let mut existing: serde_json::Value = read_json(&path)?;
    existing["providers"] = serde_json::to_value(&config.providers)?;
    atomic_write(&path, &serde_json::to_string_pretty(&existing)?)?;
    
    // 更新 settings.json 的 defaultProvider/defaultModel
    let settings_path = home.join(".pi/agent/settings.json");
    let mut settings: serde_json::Value = read_json(&settings_path)?;
    settings["defaultProvider"] = json!(config.default_provider);
    settings["defaultModel"] = json!(config.default_model);
    atomic_write(&settings_path, &serde_json::to_string_pretty(&settings)?)?;
    Ok(())
}
```

## 迁移策略

**不迁移现有数据**，直接重新开始：
1. mgr 层：清空 `models.json`，只保留空的 providers 字典
2. sandbox 层：删除 `~/.aio/models/` 下的快照文件，但保留原生配置文件
3. 用户需要重新配置所有 provider 和 agent

## 兼容性与回滚

### 版本控制
- mgr API: `/api/models/providers` 新增 v2 前缀或版本字段
- sandbox API: `/api/models/config` 保持路径不变，但响应结构改变

### 回滚方案
- mgr 层：保留旧代码分支，通过 feature flag 切换
- sandbox 层：保留旧的 render 模块，新旧并存，逐步迁移

## 性能考虑

### Usage 汇总
- mgr 定期（如每 5 分钟）轮询各 sandbox 的 `/api/models/usage`
- 缓存结果到 `mgr-data/usage_cache.json`
- 前端查询时读取缓存，避免实时请求所有 sandbox

### 供应商库缓存
- sandbox 首次访问时从 mgr 拉取供应商库
- 缓存到 `~/.aio/models/templates_cache.json`
- 提供手动刷新按钮

## 安全考虑

### API Key 处理
- mgr 供应商库中的 apiKey 需要加密存储（可选）或至少 masking
- sandbox 拉取模板时不传输完整 apiKey，只传输 masking 后的版本
- 用户在 sandbox 配置时手动输入完整 apiKey

### 文件权限
- 所有包含 apiKey 的文件设置 0600 权限（cc-switch 的 atomic_write_private）

## 实现优先级

### Phase 1: 基础设施
1. 定义新的数据模型（ProviderTemplate, ProviderSnapshot 等）
2. 实现 mgr 供应商库 API（CRUD + templates 接口）
3. 实现 sandbox 配置存储（cc-switch 模式）

### Phase 2: 后端渲染
1. 实现 claude/codex 切换式渲染
2. 实现 pi/opencode 增量式渲染
3. 实现 Usage 统计读取

### Phase 3: 前端 - mgr 供应商库
1. 重构 ModelsPage → ModelProvidersPage
2. 实现 ProviderEditorModal（中心弹窗）
3. 删除 import from pi, models.dev 相关代码

### Phase 4: 前端 - sandbox 配置
1. 在 sandbox 详情页新增"模型配置"标签
2. 实现 SwitchAgentPanel 和 IncrementalAgentPanel
3. 实现 SourceCodeEditor 组件
4. 实现 TemplatePickerModal

### Phase 5: Usage 汇总
1. 实现 mgr Usage 聚合服务
2. 实现 Usage 汇总页面

### Phase 6: 清理与文档
1. 删除废弃的 profile 系统代码
2. 删除废弃的 SandboxTable 代码
3. 更新 API 文档和前端类型定义
