# cc-switch 配置模型调研

> 源码:/tmp/cc-switch-src(github.com/farion1231/cc-switch,Tauri v2;React 在 src/,Rust 在 src-tauri/)。
> 调研日期:2026-08-26。供统一模型配置页(task 08-26-unified-model-config)借鉴其配置渲染与切换 UX。

## 1. Provider 数据模型

`src-tauri/src/provider.rs:10-44`:

```rust
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(rename = "settingsConfig")]
    pub settings_config: Value,        // 核心 payload —— 每个 agent app 形状不同
    pub website_url: Option<String>,
    pub category: Option<String>,      // official | cn_official | aggregator | third_party | custom
    pub created_at: Option<i64>,
    pub sort_index: Option<usize>,
    pub notes: Option<String>,
    pub meta: Option<ProviderMeta>,    // 仅 DB 元数据,永不写入 live 配置
    pub icon: Option<String>,
    pub icon_color: Option<String>,
    pub in_failover_queue: bool,
}
```

- `settings_config` 是不透明 JSON blob,**每个 app 一份形状**(最重要设计)。
- `ProviderManager = {providers: IndexMap<id, Provider>, current: String}` —— 每 app N 个 provider、恰好一个 current。
- 存储:`~/.cc-switch/cc-switch.db`(SQLite,providers 表,PRIMARY KEY (id, app_type),is_current 行)为主,`~/.cc-switch/config.json`(v2 JSON)为兼容层;保存前备份 `.bak`。

### 两种写入模式(`app_config.rs:417-422`)

```rust
pub fn is_additive_mode(&self) -> bool {
    matches!(self, AppType::OpenCode | AppType::OpenClaw | AppType::Hermes | AppType::Pi)
}
```

- **切换式**(claude/codex/gemini/grokbuild):live 配置文件整体由当前 provider 替换。
- **增量式**(opencode/pi/hermes/openclaw):全部 provider 共存于 agent 原生配置文件的各自键下,"切换"只是启用/指向。

## 2. 各 agent 渲染目标(verbatim 形状)

统一入口 `write_live_snapshot()`(`src-tauri/src/services/provider/live.rs:1242-1406`)。

### claude → `~/.claude/settings.json`(切换式)

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "https://api.moonshot.cn/anthropic",
    "ANTHROPIC_AUTH_TOKEN": "",
    "ANTHROPIC_MODEL": "kimi-k2.7-code",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "kimi-k2.7-code",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "kimi-k2.7-code",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "kimi-k2.7-code",
    "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "262144",
    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "262144"
  }
}
```

- auth 字段可选 `ANTHROPIC_AUTH_TOKEN`(默认)或 `ANTHROPIC_API_KEY`(meta.apiKeyField)。
- 写前 `sanitize_claude_settings_for_live` 剥内部字段;settings_config 可以搭带 settings.json 其他键(permissions/hooks…)。

### codex → `~/.codex/config.toml` + `~/.codex/auth.json`(切换式)

settings_config = `{ "auth": {...auth.json 内容...}, "config": "<整份 config.toml 字符串>" }`。TOML 模板:

```toml
model_provider = "custom"
model = "{model}"
model_reasoning_effort = "{reasoning_effort}"
disable_response_storage = true

[model_providers.custom]
name = "NewAPI"
base_url = "{codex_base_url}"
wire_api = "responses"
requires_openai_auth = true
```

- auth.json:`{ "OPENAI_API_KEY": "sk-..." }`(或 ChatGPT OAuth bundle)。
- baseURL 归一:origin-only 且不以 /v1 结尾才补 `/v1`(`provider.rs:822-834`)。
- 双文件事务:失败回滚(`CodexLiveStateSnapshot`);可选"保留 ChatGPT 登录"模式(key 写进 `experimental_bearer_token`,auth.json 不动)。

### opencode → `~/.config/opencode/opencode.json`(增量式)

settings_config = 一个 provider 片段:

```json
{
  "npm": "@ai-sdk/openai-compatible",
  "name": "Kimi",
  "options": { "baseURL": "https://api.moonshot.cn/v1", "apiKey": "", "setCacheKey": true },
  "models": { "kimi-k2.7-code": { "name": "Kimi K2.7 Code" } }
}
```

- 写入 = **读-改-合并**:`full_config["provider"][provider_id] = fragment`,原子重写,其他用户键全保留;进程级互斥锁串行(`opencode_config.rs:157-180`);JSON5 容错读。
- npm 可选 `@ai-sdk/openai` / `@ai-sdk/openai-compatible` / `@ai-sdk/anthropic` / `@ai-sdk/google` 等。

### pi → `~/.pi/agent/models.json`(增量式)

- cc-switch 只管 `models.json.providers.<key>` 显式节点;`settings.json` 的 defaultProvider/defaultModel 由 pi 自己拥有。
- settings_config 形状 = pi 的 ProviderEntry(name/baseUrl/api/apiKey/headers/compat/models)。
- 插入/替换/删除带**修订号 CAS**防外部并发修改;`atomic_write_private` 0600。

## 3. 切换/生效机制

`switch_provider` → `ProviderService::switch` → `switch_normal`(`services/provider/mod.rs:5036-5260`):

1. **backfill**:切换前读当前 live 文件,剥公共片段后存回**旧** provider 的 settings_config——用户在 agent 侧的手改随快照旅行,切回时还原。
2. **公共片段深合并**:per-app 共享 JSON/TOML 片段在写时 deep-merge 到 provider 配置之上。
3. 原子写:`file.tmp.{ts}.{pid}.{counter}` + rename;私密文件 0600;TOML 写前语法校验;codex 双文件失败回滚 auth.json。
4. 增量式 app 跳过 backfill/is_current,直接键级合并。

## 4. 模型列表与验证

- **/v1/models 拉取**(`services/model_fetch.rs`):`fetch_models(base_url, api_key, …)` 15s 超时;解析 `{data:[{id, owned_by}]}`;**多候选 URL**(`/v1/models`、`/models`、剥 anthropic 系后缀 `/anthropic|/claude|/api/coding` 后重拼,`:207-263`);preset 可用 `modelsUrl` 覆盖端点。每个表单有 "fetch models" 按钮填充下拉。
- **连通性检测**(`services/stream_check.rs:89-157`):重试仅限超时,返回 `{status, success, message, responseTimeMs, httpStatus}`,驱动 `ProviderHealthBadge`。
- **测速**(`services/speedtest.rs`):并行 HTTP GET 延迟;`meta.endpointAutoSelect` 自动选最快端点。
- **预设即数据**:per-app 精选数组(完整 settingsConfig + 端点候选 + 模型目录含 contextWindow/reasoningLevels);`UniversalProvider` 是跨 app 预设(一份 baseUrl/key + per-app 生成器),最接近"统一配置页"的概念。

## 5. UI/UX 结构(可借鉴)

- App 切换 tab 选 agent;主视图 provider 卡片列表(图标/名称/分类徽章/当前徽章/健康徽章;行操作:一键切换、编辑、复制、删除)。
- 新增/编辑 = 预设选择器 + 结构化表单(baseUrl/apiKey/model 下拉带拉取按钮 + 测速)+ 折叠"高级"(有值自动展开)+ **原始 JSON 编辑器**(settingsConfig 的逃生舱)。
- 从 live 导入:读现有 ~/.claude/settings.json 等作为新 provider 快照。

## 6. 对本任务的取舍

- **采纳**:双写入模式分类(切换式 vs 增量式)、原子写+备份、/v1/models 多候选 URL 与解析、连通检测返回形状、per-agent 覆盖项的存在(claude 三档映射、codex effort)。
- **不采纳**:per-app settingsConfig blob 存储(与"统一供应商库+每agent分配"范式冲突——我们用 canonical schema 派生渲染)、backfill 快照机制(canonical 库是 SSOT,采用键级合并保护用户键即可)、SQLite provider 库(单用户场景一个 JSON 文件足够)。
