# 供应商 id 从名称派生 + 迁移 provider-1/2（pi 全家 UI 显示 id 而非 name）

## Goal

用户在供应商库填写的名称（deepseek/anyrouter）从未体现在 pi 界面——上一单写入 pi 节点 `name` 后依旧显示 `provider-1`。已验证根因：**pi 上游全部 UI 展示 provider id 而非 name**（pi TUI 选择器徽标 `[id]` `model-selector.ts:323`、TUI footer `(id)` `footer.ts:193`、pi-web 分组头 `ModelSelector.tsx:293`，且 pi-web `/api/models` 只透出 id）。治本 = 让 id 本身有意义。

## Requirements

### A. 前端：id 从名称派生（未来供应商）

- `types.ts`：新增 slug 派生（ascii 小写字母/数字/连字符；非法字符→`-`；折叠重复、去首尾；空结果回退占位 id），对既有 id 去重（`-2`/`-3` 后缀）。
- 名称变更时（ModelsPane updateProvider 路径）：id 仍为 `^provider-\d+$` 占位且 slug 非空 → re-key 为 slug(name)，并同步修 `agents.pi/opencode.provider`、`agents.claude/codex.presets[].provider` 引用。已自定义 id 不受影响（迁移后的 deepseek/router 不会被再次改名）。
- 中文/特殊字符名称 slug 为空 → 保持占位 id。

### B. 一次性迁移（现有数据）

- canonical `~/.aio/models.json`：providers 键 provider-1→deepseek、provider-2→router（取各节点 name）；`agents.pi.provider`、`agents.opencode.provider`、`agents.claude.presets[].provider` 同步改。
- live 清理：pi `models.json` 删 `provider-1` 旧节点（走既有 delete 路由，含 settings defaultProvider 悬空清理）；opencode.jsonc 若有 provider-1 fragment 同样删。
- re-apply pi + opencode；claude 预设仅存引用、settings.json 不含 id，无需 apply。
- 副作用（已告知用户）：旧 pi 会话内记录的 provider-1 无法解析，新会话无影响。

## Acceptance Criteria

- [ ] 前端 build（tsc + vite）通过。
- [ ] 迁移后 canonical 无 provider-1/2 且引用完整；pi models.json 只有 `deepseek` 节点且含 `"name": "deepseek"`、settings defaultProvider=deepseek；opencode 无 provider-1 残留。
- [ ] pi 实测跑通请求；pi-web `/api/models`（60s 缓存过期后）分组头显示 deepseek。
- [ ] 派生逻辑：名称 deepseek → id deepseek；重名 → `-2` 后缀；名称清空不动 id；自定义 id 不被覆盖。

## Notes

- pi-web 模型缓存 TTL 60s（pi-web lib/models-cache.ts），迁移后无需重启。
- `genProviderId` 占位逻辑保留：新增供应商时 name 为空，无法立即派生。
- pi 上游 name 字段的展示缺失不改我们代码（name 仍写入，语义正确）。
