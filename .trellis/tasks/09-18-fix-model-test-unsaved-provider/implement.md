# 执行计划:fix-model-test-unsaved-provider

前置:PRD 已定稿(R1–R4)。本任务为跨 mgr 后端 + mgr-web 前端的小型修复,
按以下顺序执行,每步带验证点。

## 步骤

1. **后端:mgr/src/models.rs — TestRequest 改 untagged 枚举**
   - 仿照 `DiscoverRequest`(同文件 :859)新增:
     ```rust
     #[derive(Debug, Deserialize)]
     #[serde(untagged)]
     #[allow(non_snake_case)]
     enum TestRequest {
         ById { providerId: String, modelId: String, #[serde(default)] protocol: Option<String> },
         Literal {
             baseUrl: String,
             #[serde(default = "default_api")] api: String,
             apiKey: Option<String>,
             modelId: String,
             #[serde(default)] protocol: Option<String>,
         },
     }
     ```
   - handler `test()` 里把"取 provider"改成分支:ById 走现有存储解析(行为
     不变);Literal 构造临时 provider(`base_url/api_key/headers 为空`),其后
     探测代码完全复用。Literal 空 baseUrl → 400(对齐 discover 的
     resolve_provider 空白校验)。
   - 验证:`cargo test -p mgr`(/ `cargo test --workspace`)。

2. **后端:单测**
   - 反序列化:`{providerId, modelId}` → ById;`{baseUrl, modelId}` → Literal
     (api 默认 openai-completions);两种形态带 protocol。
   - 行为:literal 无 apiKey → `{ok:false, error:"No API key found..."}`。

3. **前端:mgr-web/src/api.ts — testModel 增加 literal 形态**
   - 对齐 `discoverModels` 的联合入参:
     `{providerId, modelId, protocol?} | {baseUrl, api, apiKey?, modelId, protocol?}`。
   - JSDoc 注明语义(与 discoverModels 的注释风格一致)。

4. **前端:ModelsPage.tsx — handleTest 选择形态**
   - 取 `config?.providers[providerId]` 判存在性;不存在(dirty 中的新
     provider)或 `apiKeyDirty`(重新输入过 key)→ literal 形态(传
     `provider.baseUrl / provider.api / provider.apiKey`,掩码 key 不传);
     否则 ById 形态。与 `handleFetchModels` 的判断保持一致语义。
   - `handleTest` 依赖数组补齐(现在只有 `[profileId]`,需要 config/selectedId
     或用 ref 取最新值,注意避免 stale closure)。

5. **手动验证**
   - `make` 正常起栈(mgr.localhost)→ 新建供应商 → discover 选模型 → 不保存
     点「检测」→ Network 面板确认 literal 请求体、pill 显示真实结果。
   - 已保存供应商回归:请求体仍 `{providerId, modelId}`。

6. **构建门禁**
   - mgr-web:`tsc --noEmit && vite build`。
   - Rust:`cargo fmt` + `cargo clippy` + `cargo test`。

7. **Spec 回写**(Phase 3.3)
   - `.trellis/spec/backend/model-config-guide.md` "Discover & test" 段落:补
     test 的 literal 形态与语义。

## 回滚点

- 全部改动集中在 3 个文件 + spec 回写,单 commit,出问题直接 revert。

## 决定记录

- app 侧 `app/src/routes/models/test.rs` 同样是 providerId-only,但 mgr-web
  只走 mgr 路由;app 侧不对齐(避免无调用方的契约膨胀),如未来 app 直连
  需要再说。
