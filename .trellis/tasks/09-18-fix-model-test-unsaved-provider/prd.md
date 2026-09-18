# 修复未保存供应商的模型检测立即失败(test 支持 literal 请求体)

## Goal

Issue #23:在 mgr-web「模型」页新建(或编辑后未保存)的供应商上点模型「检测」,
后端只按 `providerId` 查 kv 存储里的已保存配置,查不到立即 404,前端 pill 立刻
显示失败——真实探测请求从未发出。保存后重进再点检测才正常。

`/api/models/discover` 已通过 untagged 枚举支持 literal 请求体
(`{baseUrl, api?, apiKey?}`)解决同一问题;本任务把 test 链路对齐。

## Requirements

- **R1 后端 literal 分支**:`POST /api/models/test` 的请求体改为 untagged 枚举,
  保留现有 `{providerId, modelId, protocol?}` 形态(行为不变),新增 literal 形态
  `{baseUrl, api?, apiKey?, modelId, protocol?}`——直接用传入字段构造 completion
  探测,不查存储。`api` 缺省沿用 discover 的 `"openai-completions"` 默认值。
- **R2 后端行为约束**:literal 分支复用现有探测实现(TEST_TIMEOUT 20s、
  PROBE_PROMPT、MAX_OUTPUT_TOKENS 16、completion_url/build_headers/
  completion_body、响应截断),两分支只有"provider 字段来源"不同。错误契约不变:
  无 key → HTTP 200 `{ok:false, error}`;其余探测失败同样 200 + ok:false。
- **R3 前端字面量传递**:`mgr-web` 的 `testModel` 增加 literal 入参;
  `ModelsPage.handleTest` 在供应商处于未保存状态(dirty,或 providerId 不在已
  保存配置中)时传字面量 `{baseUrl, api, apiKey, modelId}`,判断方式与
  `handleFetchModels` 的 `apiKeyDirty` 逻辑对齐;key 为掩码(`****`)时不得把
  掩码当真实 key 发出去。
- **R4 测试 pill 语义**:未保存供应商的检测仍写入 `testState`(key 仍用
  `providerId:modelId`),现有"识别字段变化即重置 pill"的 effect 不需改语义。

## 约束

- wire 契约向后兼容:老形态 `{providerId, modelId}` 必须原样可用(mgr 是唯一
  调用方,但 app 侧同名路由若存在同样形态则保持不动)。
- 不改 discover 的行为与契约。

## Acceptance Criteria

- [ ] 新建供应商(填 URL/key、从 discover 选模型、**不保存**)→ 点该模型
  「检测」:发出真实的 completion 探测请求,pill 显示 ok/fail 与延迟,而非立即
  失败。(手动验证路径,可在浏览器 devtools Network 看到 `/api/models/test`
  请求体为 literal 形态。)
- [ ] 已保存供应商点「检测」:请求体仍为 `{providerId, modelId}` 形态,行为与
  现状一致(回归)。
- [ ] literal 形态缺少 apiKey 时:返回 `{ok:false, error:"No API key found..."}`
  语义(与 ById 分支一致),HTTP 200。
- [ ] `cargo test`(mgr)与 `mgr-web` 的 `tsc --noEmit && vite build` 通过;
  后端为 untagged 两分支各补至少一个反序列化/行为单测。
- [ ] `.trellis/spec/backend/model-config-guide.md` 的 test 契约段落回写
  literal 形态。

## Notes

- 涉及文件:`mgr/src/models.rs`(TestRequest + test handler)、
  `mgr-web/src/api.ts`(testModel)、`mgr-web/src/pages/models/ModelsPage.tsx`
  (handleTest)。
- 实施时已确认:app 侧 `app/src/routes/models/test.rs` 存在同样的
  providerId-only `TestRequest`(其 discover 的 literal 分支在 discover.rs
  内部,不入 test.rs)。mgr-web 只调用 mgr 的路由,本任务修 mgr 侧;app 侧
  test 是否对齐 literal 留作独立决定(在 implement.md 记录取舍)。
