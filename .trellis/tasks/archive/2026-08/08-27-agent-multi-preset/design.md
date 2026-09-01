# Design — claude/codex 多配置项(cc-switch 式 preset)

> prd.md 见同目录。范式:**preset 派生自统一供应商库**(providerId + model + 覆盖项),
> 不复制 settingsConfig blob;不做 backfill 快照(canonical 是 SSOT,键级合并已保护用户键)。

## 1. canonical schema(向后兼容迁移)

### 新形状

```rust
// store.rs
pub struct AgentsConfig {
    pub pi:       Option<AgentAssignment>,      // 不变(pi/opencode 保持单分配,见 §5)
    pub opencode: Option<AgentAssignment>,      // 不变
    pub claude:   Option<ClaudePresets>,        // ClaudeAssignment -> ClaudePresets
    pub codex:    Option<CodexPresets>,         // CodexAssignment  -> CodexPresets
}

#[serde(rename_all = "camelCase")]
pub struct ClaudePresets { presets: Vec<ClaudePreset>, current: Option<String> }

#[serde(rename_all = "camelCase")]
pub struct ClaudePreset {
    id: String, name: String,
    provider: String, model: String,
    haiku_model: Option<String>, sonnet_model: Option<String>, opus_model: Option<String>,
    auth_field: String,             // 默认 "AUTH_TOKEN"(沿用 default_auth_field)
}
// CodexPresets / CodexPreset 同构:id, name, provider, model, reasoning_effort?, wire_api
```

### 兼容迁移:shadow 反序列化

GET/PUT 都走整份 `CanonicalConfig` serde,迁移收敛在反序列化层:

```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudePresetsShadow {
    #[serde(default)] presets: Vec<ClaudePreset>,
    #[serde(default)] current: Option<String>,
    // 旧单 assignment 字段(presets 缺失时视为迁移输入)
    #[serde(default)] provider: Option<String>,
    #[serde(default)] model: Option<String>,
    #[serde(default)] haiku_model: Option<String>,
    #[serde(default)] sonnet_model: Option<String>,
    #[serde(default)] opus_model: Option<String>,
    #[serde(default)] auth_field: Option<String>,
}
impl From<ClaudePresetsShadow> for ClaudePresets {
    fn from(s) -> Self {
        if !s.presets.is_empty() {
            return ClaudePresets { presets: s.presets, current: s.current };
        }
        // 旧形状:整体当一条 preset,置为 current
        if let Some(provider) = s.provider.filter(|p| !p.is_empty()) {
            let preset = ClaudePreset {
                id: "default".into(), name: "默认配置".into(),
                provider, model: s.model.unwrap_or_default(),
                haiku_model: s.haiku_model, sonnet_model: s.sonnet_model,
                opus_model: s.opus_model, auth_field: s.auth_field.unwrap_or_else(default_auth_field),
            };
            return ClaudePresets { presets: vec![preset], current: Some("default".into()) };
        }
        ClaudePresets::default()   // 两者皆空 -> 空预设
    }
}
```

- serde 默认忽略未知键:旧文件 `anthropic` 等残留照旧丢弃;**新形状序列化只出 presets/current**,
  旧字段随下次保存自然消失(与 R1 同一策略)。
- shadow 同时吃「新形状 PUT」与「旧形状读盘」;前端只发新形状。
- **不引入 version 字段**:迁移是形状级无损的(单 assignment ⊂ presets),无双向写回风险。

## 2. preset id 规则

- 后端生成:kebab 短 id(`preset-a1b2c3`,4-6 位随机后缀),保证 agent 内唯一;创建时校验
  撞名重生成。前端新增 preset 不自带 id,PUT 时后端为无 id 的 preset 补 id(或前端用
  crypto.randomUUID 短截——**选后端补 id**,前端零假设)。
- 删除当前 preset:current 顺移到剩余首项;presets 空 -> current=None。

## 3. 渲染器(apply 当前 preset)

- `apply_claude`:`let Some(assignment) = canonical.agents.claude.as_ref()
  .and_then(|p| p.current.as_ref()).and_then(|id| p.presets.iter().find(|x| x.id == *id))`
  取当前 preset;其余逻辑不变。找不到 -> push_err「no current claude preset」,不写半截。
- `apply_codex` 同构。
- 校验(validate_assignment):对**每个** preset 校验 provider/model 存在性(未选中的
  preset 也别是坏的),错误信息带 preset name。

## 4. 前端(panes/models/)

- `types.ts`:`ClaudePresets/ClaudePreset/CodexPresets/CodexPreset` 接口 + 解码;
  `AgentAssignment`(pi/opencode)不动。
- `AgentTabs.tsx` 按 agent 分叉:
  - pi/opencode:保持现表单(其升级归 08-27-agent-tabs-live-config)。
  - claude/codex:新组件 `PresetList`(可放同文件或独立 `PresetList.tsx`)——卡片列表
    (name / model / 协议徽标 / 「当前」徽标),行操作:设为当前(主操作,触发保存+apply)、
    编辑(行内展开或小抽屉)、复制、删除。
  - 新增/编辑表单:provider select(兼容过滤沿用)→ ModelPicker 复用该供应商 models[]
    → 覆盖项(claude 三档 + authField / codex effort + wireApi)。
- `ModelsPane.tsx`:agent 状态从 `Record<AgentTab, Assignment>` 调整为分型;新增 handler:
  `addPreset/updatePreset/deletePreset/duplicatePreset/setCurrentPreset`(全部走现有
  PUT canonical 通道,不新增后端路由;apply 沿用 `POST /api/models/apply/:agent`)。
- 复制 preset:新 id + name 加「副本」后缀,插到源后面。
- i18n:新增 preset 相关键(当前/设为当前/复制/删除确认/新配置/默认配置…),zh+en。

## 5. 明确不做

- pi/opencode 不改多 preset:它们是增量式(多 provider 共存于原生文件),「切换」由
  defaultProvider/defaultModel 承担,多 preset 无语义;其页签增强归
  08-27-agent-tabs-live-config。
- 不做 preset 内嵌独立 apiKey/headers:凭据一律来自供应商库(SSOT);需要不同 key =
  建另一个供应商(与 cc-switch 的 settingsConfig blob 语义不同,已在 PRD 护栏声明)。

## 6. 兼容矩阵

| 输入 | 行为 |
| --- | --- |
| 旧 models.json 单 assignment | 读 -> presets=[default],current=default;保存落新形状 |
| 新形状 presets+current | 原样读 |
| presets 空 + current 有值 | 读正常;apply 报「no current preset」(validate 在 PUT 时即报) |
| 前端旧缓存(SPA 未刷新) | 不考虑(同仓同发) |

## 7. 测试策略

- store:shadow 迁移单测(旧形状/新形状/空/缺 model 的旧形状/presets 空但 provider 在)。
- claude/codex renderer:多 preset 下 apply 写 current;current 指向不存在 id -> err;
  current=None -> err;删除顺移逻辑(若在后端,单测)。
- validate:非当前 preset 引用未知 provider -> PUT 报错带 preset name。
- 前端:`npm run build`;PresetList 交互目测(容器实测)。
