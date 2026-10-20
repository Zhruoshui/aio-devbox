# Research findings (2026-10-13)

## F1. bigmodel anthropic discover 根因（实测）

用例：`api=anthropic-messages`，`baseUrl=https://open.bigmodel.cn/api/anthropic`，key 见任务描述。

curl 实测（x-api-key + anthropic-version: 2023-06-01）：

| 候选 URL（candidate_urls 顺序） | HTTP | body |
| --- | --- | --- |
| `…/api/anthropic/models?limit=1000`（primary） | **200** | `{"code":500,"msg":"404 NOT_FOUND","success":false}` |
| `…/api/anthropic/v1/models` | 200 | 合法 `{"data":[{"id":"glm-4.6","display_name":"GLM-4.6",…}]}` |
| `…/api/paas/v4/models`（openai 协议路径） | 200 | 合法 `{"object":"list","data":[…]}` |

根因：discover 处理器对「2xx 但 parse_discovered_models 为空」**立即 return BAD_GATEWAY**，不再尝试后续候选 → 排第二的可用候选 `/api/anthropic/v1/models` 永远轮不到。

同源代码两处，需同步修：
- `mgr/src/models.rs` discover()（~L698-717）
- `app/src/routes/models/discover.rs`（~L120-140，byte-for-byte 同款）

修法：2xx-无模型改为 `continue` 并记录 `no_models` 错误；全部候选耗尽后按「auth 失败 > no models > 全失败」优先级返回。两处各已有纯函数单测可扩。

## F2. cc-switch 文件对齐（源码考证，v3.x）

浅克隆 `farion1231/cc-switch`（/tmp/cc-switch）确认各 agent 管理的原文件：

| agent | 原文件 | 说明 |
| --- | --- | --- |
| claude | `~/.claude/settings.json` | provider env 配置本体（另有 `~/.claude.json` 为 MCP 管理面，与本任务无关） |
| codex | `~/.codex/config.toml` + `~/.codex/auth.json` | 两文件都由 snapshot 的 `{config, auth}` 承载 |
| pi | `~/.pi/agent/models.json` + `~/.pi/agent/settings.json` | cc-switch 只管 models.json 的显式 provider 节点；settings.json 存 defaultProvider/defaultModel（"Pi owns account login and the active provider/model in settings.json"） |
| opencode | `~/.config/opencode/opencode.jsonc`（回退 `opencode.json`） | jsonc 允许注释（cc-switch 有 jsonc_document.rs） |

与本项目 app 渲染器对照：claude/codex/opencode 完全一致；**pi 渲染器已写 models.json+settings.json 两文件**（render/pi.rs），但 mgr-web 的整文件预览把 providers+defaults 拼成**一个**假 JSON（IncrementalAgentPanel.mergedPreview）——与真实落盘不符，需在 R6 修正为两文件分别展示/编辑。

## F3. 现有代码事实（改动落点）

- **模板 apiKey 剥离点**：`mgr/src/models.rs get_templates()`（L514-547）json! 里根本没有 apiKey 字段；`ProviderTemplatePublic`（mgr-web types.ts L71）无 apiKey；TemplatePickerModal 注释明言 10-04 PRD 的"不传 key"决定——本次需求显式推翻它（mgr-web 是唯一管理面，与供应商库编辑器同信任域）。
- **SourceCodeEditor**（components/SourceCodeEditor.tsx）：已支持 view/edit 双模式 + JSON 校验 + TOML 直通；语言枚举 `json|toml`，需扩 `jsonc`。
- **SwitchAgentPanel**：claude/codex 源码编辑已存在（snapshotToSource/sourceToSnapshotSettings，sandboxTypes.ts L338-365）。**codex 源码只覆盖 config.toml，auth.json 无源码入口**（form 里有 auth textarea）。
- **IncrementalAgentPanel**：整文件视图 `mergedPreview` 只读（L435-444）；节点编辑器无「获取模型」按钮。
- **discover 多选对话框**：ProviderEditorModal 已有（checkbox 多选+过滤+计数+添加所选，slice 200）；因 F1 用户从未见过 anthropic 供应商成功拉取。缺全选/清空快捷操作。
- **卡片溢出**：`.sbx-card`（components.css L349-358）内 URL 为 `<span class="muted txs mono">`，无 ellipsis/break；`.pv .url`（styles.css L1929）有处理。
- **mgr-web 与 mgr 同源**：mgr-web 由 mgr 服务，前端可直接调 mgr `/api/models/discover`（literal 分支），无需绕沙箱代理——增量面板加「获取模型」走这条。
- dist 为 10-10 构建；交付时需 `npm run build` 刷新 dist（mgr 容器即取此目录）。

## F4. 网络备注

- bigmodel `open.bigmodel.cn` 在本沙箱防火墙允许（curl 直连成功）；app/mgr 侧 discover 由部署环境出网，同样可达。
