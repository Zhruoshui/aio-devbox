# 修复 pi provider 显示名：渲染 name 字段（provider-1 → deepseek）

## Goal

用户在供应商库填写了供应商名称（canonical `provider-1.name = "deepseek"`、`provider-2.name = "anyrouter"`），但 pi 侧始终显示 `provider-1 / deepseek-v4-flash-vision-exp` 这类 id 名。根因：`render_pi_provider`（`app/src/routes/models/render/pi.rs`）基于错误断言"pi 原生 provider 节点没有 name 字段"完全不渲染 `name`。修复后 pi 显示 `deepseek / deepseek-v4-flash-vision-exp`。

## pi 侧事实（已对照 earendil-works/pi 源码验证）

- `ProviderConfigSchema`（`packages/coding-agent/src/core/model-config.ts:197`）：`name: Type.Optional(Type.String({ minLength: 1 }))` —— 节点**支持** name，且**不能是空串**（minLength 1）。
- 显示名解析链（`packages/coding-agent/src/core/provider-composer.ts:478`）：`name: extension?.name ?? config?.name ?? base?.name ?? ... ?? providerId`，`config` 即 models.json provider 节点 —— 写了就显示，缺省回落 id。

## Requirements

- `render_pi_provider`：canonical `name` 非空时写 `name` 字段；为空不写（omitted-empty 惯例不变，`anthropic` 等仍不写）。
- `edit_pi_provider`（live 编辑）：接受 `patch.name` —— 非空写入、空串**删键**（minLength 1，写空串会让整个 models.json schema 失效）。
- 修正两处错误注释：`render_pi_provider` 文档（"no `name` ... pi doesn't use them"）、`edit_pi_provider` 文档（"`patch.name` is ignored for pi"）。
- 测试翻转：`provider_renders_omit_empty_fields`（有 name 应写出）、`edit_pi_provider_merges_fields_only`（name 应写入）；新增空 name 省略用例。
- 不改 canonical/store/前端：名字在 canonical 侧完好，纯渲染层丢失。

## Acceptance Criteria

- [ ] `cargo test` 全绿（含翻转后断言与新增空 name 用例）。
- [ ] 重建部署后 `POST /api/models/apply/pi`，`~/.pi/agent/models.json` 的 `provider-1` 节点含 `"name": "deepseek"`。
- [ ] pi 加载 models.json 无 schema 报错（沿用上单验证法：pi 实际发起请求而非报 config 错误）。
- [ ] 其余节点与未知键保持原样（golden 测试覆盖）。

## Notes

- 上游归属：该错误断言同样源自 08-27-provider-form-piweb 设计期的调研误判；spec `model-config-guide.md` 相关行本单同步回写。
- 空串语义与 apiKey 对齐（"" = 清除），但动机不同：apiKey 清除是 canonical 掩码规则，name 删键是 pi schema minLength 约束。
