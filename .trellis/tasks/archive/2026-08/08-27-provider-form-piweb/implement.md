# Implement - 供应商表单 pi-web 流 + models.dev 集成

> 前置:读 `prd.md`、`design.md` §0-§6。改动顺序:后端 catalog + pi cost 修复 ->
> 后端测试绿 -> 前端 ModelPicker/ModelRow -> 抽屉重构 -> models.dev 填充接线 ->
> i18n/样式 -> 容器手测 -> 收尾。

## 1. 后端 catalog 路由

- [x] 新文件 `app/src/routes/models/catalog.rs`:`CatalogModel`/`CatalogProvider`/
      `CatalogResponse`(design §1.1);`get_catalog` handler,复用 `AppState` 的
      `reqwest::Client`,15s 超时抓 `https://models.dev/api.json`。
- [x] 缓存:`OnceLock<Mutex<Option<CatalogCache>>>`,1h TTL,持锁内 fetch 做
      in-flight 去重(design §1.2)。
- [x] 解析容错:`Value` 手动挑字段,缺失字段 `None`,顶层结构不符 -> 502(不 panic)。
- [x] 注册路由:`main.rs` 加 `get(get_catalog)` -> `/api/models/catalog`;
      `routes/models/mod.rs` 或专属 pub use 导出。

## 2. 后端 pi cost 单位修复

- [x] `render/pi.rs::render_pi_cost`:每个非 None 子项除以 `1_000_000.0` 再写出
      (design §0)。更新/新增单测:canonical `input=0.14` -> pi 侧
      `cost.input≈0.00000014`(浮点近似断言)。
- [x] 检查 `render/pi.rs` 现有 cost 相关单测(`model_cost_renders_camel_case` 等)
      期望值同步更新为除以 1e6 后的值。

## 3. 后端测试

- [x] catalog 归一化:mock JSON(内联 fixture 字符串)-> 断言 `CatalogResponse`
      字段映射;providers/models 数量、cost 透传。
- [x] 缓存命中:两次调用间隔 < TTL,断言底层 fetch 只触发一次(用可注入的测试
      client 或计数标记 —— 参照 discover.rs/test.rs 现有测试对 reqwest 的处理方式,
      若已有 mock-server 依赖复用;否则用超时/错误路径可测的部分先覆盖,缓存命中
      逻辑用一个纯函数抽出可单测的 TTL 判断)。
- [x] 超时/非 2xx -> 502 + 截断 500 字符(仿 discover.rs 现有断言写法)。
- [x] `render_pi_cost` 单位修复单测(见 §2)。
- [x] `docker run --rm -v $PWD/app:/app -v aio-cargo-registry:/usr/local/cargo/registry
      -v aio-cargo-cache:/usr/local/cargo -w /app rust:1-bookworm cargo test models`
      全绿。

## 4. 前端:ModelPicker + ModelRow

- [x] `web/src/panes/models/ModelPicker.tsx`:无状态纯 props 组件(design §2),
      本任务内只在 §5 的填充流程消费其挑选能力雏形(若本任务不需要它做独立 UI,
      仅实现最小可复用形状,留给 R2/R3 接线;不做超出 R1 verification 范围的
      交互)。
- [x] `web/src/panes/models/ModelRow.tsx`:折叠/展开单模型行(design §3),迁移
      `ModelTable.tsx` 的字段编辑 + test pill 逻辑;`onPatchModel`/`onDeleteModel`/
      `onUpdateCost`/`onTest`/`onResetTest` props 签名复用 `ProviderEditor` 现有
      handler(签名不变,只换渲染形态)。
- [x] 删除 `ModelTable.tsx`(职责被 `ModelRow.tsx` 取代);检查无残留 import。

## 5. 前端:抽屉重构 + models.dev 填充接线

- [x] `types.ts` 新增 `CatalogModel`/`CatalogProvider`/`CatalogResponse` 类型 +
      `decodeCatalog` 解码函数(单一解码边界,同 `decodeUsage` 模式)。
- [x] `types.ts` 新增纯函数 `catalogRecommend(catalog, baseUrl, api, modelId)`
      (design §4.2:host 匹配 + api 粗过滤 + model id 精确匹配,大小写不敏感)。
- [x] `ModelsPane.tsx`:懒加载 catalog 状态(首次填充按钮点击才 fetch,内存缓存
      整份不重复请求);新增 `handleCatalogFill(providerId, idx)` handler,调用
      `catalogRecommend` 命中后一次性 `onPatchModel` 六项。
- [x] `ProviderEditor.tsx`:模型接入区改为渲染 `ModelRow` 列表;「模型接入区」
      与「供应商信息区」标题/分隔按 design §3 两区结构调整(若现有 DOM 结构已是
      两段,只需替换模型列表渲染部分,不用整体重排)。
- [x] `ModelRow.tsx` 展开态接「从 models.dev 填充」按钮:未命中/请求失败时置灰 +
      `title` 提示(design §4.4),不抛错弹窗。

## 6. i18n + 样式

- [x] 新 i18n 键(zh+en):模型行折叠/展开态标签、models.dev 填充按钮文案、
      未命中/失败提示文案。
- [x] `styles.css`:`ModelRow` 折叠/展开卡片样式(Kumo `ml-*` token,仿现有
      `ml-drawer`/`ml-advanced` 折叠样式复用,不新引入非 ml- 前缀类)。

## 7. 验证(质量门)

- [x] `cargo test models` 全绿。
- [x] `cd web && npm run build` 干净。
- [x] 容器手测(design §6):重建 app 镜像 + force-recreate;新增供应商 ->
      拉取模型 -> 勾选加入 -> 展开某模型 -> 点「从 models.dev 填充」-> 数值落表单
      正确;未命中模型点填充 -> 按钮置灰提示、不报错;保存 -> apply 到 pi ->
      pi 的 `models.json` 里 cost 字段是 USD/token 量级(不是 $/M 量级)。
- [x] 对照 prd.md AC 逐项打勾;`trellis-check` 复查。

## 8. 收尾

- [x] 更新 `.trellis/spec/backend/model-config-guide.md`:新增 catalog 路由说明、
      pi cost 渲染单位修复记录;前端 spec 段同步 `ModelRow`/`ModelPicker` 组件拆分。
- [x] journal-1.md 当日进展。
- [x] commit;归档子任务;回父任务标记 R1(注明 ModelPicker 已备好供 R2/R3)。
