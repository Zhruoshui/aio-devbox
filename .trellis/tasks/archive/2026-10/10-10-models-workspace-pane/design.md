# Design — 供应商库入口修复 + 模型配置迁移工作区 pane

## D1: R1 空库入口（小修）

`mgr-web/src/pages/models/ModelsPage.tsx` 渲染分支：

```tsx
// 现状（~line 400）：空库 → 一行 muted 文案；ProviderGrid（含新增卡片）不渲染
{Object.keys(library.providers).length === 0 ? <p>...</p> : <ProviderGrid ... />}
```

改为**无条件渲染 ProviderGrid**（网格本身对空 providers 列表安全，尾部幽灵卡片即新建入口）。
`mcNoProviders`（i18n.ts zh 369 / en 918）随空态分支一起删除——文案里"或从 pi 导入"引用的正是 D6 已删功能。
全库 grep 确认无其他 "pi 导入" 残留。

## D2: R2 pane 注册方式 —— mgr-web 客户端合成，不走 services.toml

三个候选：

| 方案 | 说明 | 结论 |
|------|------|------|
| A. services.toml 加 `type=page` 条目 | 官方机制，但 `include_str!` **构建时烘进 app 镜像**；每个已建沙箱有自己的 `sandbox-app-{short}` 镜像，全部要重建才生效 | ✗ |
| B. app 侧新静态页 + gateway 路由 + iframe | "沙箱自服务"字面解，但要**整套复制** cc-switch UI（panels/编辑器/图表）成第二份代码，维护双份 | ✗ |
| C. mgr-web 在 manifest 解码后**合成** page 条目，原生 React pane 复用 `SandboxModelConfigTab` | 零后端/镜像改动；组件已是 `(sandboxName, sandboxStatus, lang)` 接口，数据仍走 `/api/sbx/:name` 代理；对旧镜像沙箱立即生效 | ✓ |

选 C。`type=page` 渲染链路两端本就健在（app `config.rs:370` 恒 enabled；mgr-web `types.ts` 仍解码 `"page"`），只是被树过滤 + pane 占位——本任务把它接回来，且**条目由客户端注入**，services.toml 不加烘焙条目。

合成条目（WorkspacePage 取到 manifest 后注入，或在 `api.ts` decodeManifest 后统一处理）：

```ts
{ id: "modelsConfig", type: "page", enabled: true, label: t(lang, "edTabModels") }
```

- 只对**运行中**沙箱生效（manifest 本就只为运行中沙箱拉取）——停机沙箱无 pane 可开，与 code-server 行为一致。
- label 复用 `edTabModels` i18n key（中英文随 lang）。
- 注入点做**去重**：若未来某沙箱镜像的 services.toml 真带了 `modelsConfig`，不重复注入。

## D3: 文件级改动清单

mgr-web（全部）：

1. `pages/models/ModelsPage.tsx` — 删空态分支，恒渲染 ProviderGrid。
2. `i18n.ts` — 删 `mcNoProviders`（zh/en）；`edTabModels` 保留但消费点从 EditPage 移到合成条目。
3. `pages/workspace/SandboxTree.tsx` —
   - `:136` 过滤 `s.type !== "page"` 改为放行（连同 :125 注释更新："page 条目由 mgr-web 客户端合成（modelsConfig），services.toml 仍不烘焙"）；
   - 图标映射（:446）`page → "sliders"` 已存在，不动。
4. `pages/WorkspacePage.tsx` —
   - manifest 拉取后注入合成条目（含去重）；
   - `PaneForService`：`type === "page"` 分支按 service.id 渲染 `SandboxModelConfigTab`（sandboxName=pane 的 sandbox；sandboxStatus 从现有沙箱列表 state 取；lang 同 XtermPane 模式）。渲染进 `pages/workspace/panes/` 新薄包装 `ModelsPane.tsx`（与 IframePane/XtermPane 同级，内部就是 Tab 组件 + pane 内边距适配）。
   - 旧布局恢复路径（componentState 里解码出 page 条目）原是防御分支，现在变成真实路径，无需额外改动。
5. `pages/EditPage.tsx` — 删「模型配置」Tab 按钮、tab 分支与 `SandboxModelConfigTab` import；Tab 栏只剩 env 一项时**整栏移除**（单 Tab 无意义，内容直出）。
6. CSS — cc-switch 表单原在 1200px `.page` 容器内，pane 可能更窄：`ModelsPane` 外层加滚动 + 最小宽度保护（`components.css` 现有 dialog/menu 习惯，不新造 token）。

文档（mgr-web 内注释 + spec）：

7. `app/services.toml:125` 退役注释更新：page 渲染链路复活于 mgr-web，条目客户端合成，仍**不要**在此文件加 page 条目（镜像烘焙对存量沙箱无效）。
8. `.trellis/spec/frontend/index.md`「Pane types」节：删除 "page is RETIRED…Do not re-introduce"，改写为合成条目机制（Phase 3.3 执行）。

## 边界与兼容

- **旧沙箱镜像**：合成条目不依赖镜像内 services.toml → 存量沙箱照常出现按钮；其 `/api/models/*` 早已存在（cc-switch Phase 2+）。
- **停机沙箱**：树里不出现模型配置按钮（manifest 不拉取），不会产出死 pane。
- **布局持久化**：golden-layout 里 page 条目进 componentState 持久化；lang 切换后 label 取的是注入时快照，刷新后自愈（可接受，与 piWeb 条目 url 同类限制）。
- **回滚**：纯前端单 commit revert + 重发静态产物即可，无数据/契约影响。

## 明确不做

- 不动 mgr/app 后端 API、代理、Caddyfile、compose。
- 不动配置存储（`/root/.aio/models/config.json` 与渲染产物）。
- 不做供应商库与沙箱配置的联动变更（模板下拉现机制保持）。
- Usage 页不动。

## 实现期修正（存档记录）

- SandboxTree `buttonsOf` 原无 page 桶——只删过滤会让条目静默消失；已加桶（顺序 web → agent → page → 自定义）。
- `gl-kumo.css` 缺 `[data-icon="sliders"]` tab 字形 mask（page 类型此前不可能成为 tab）——check 阶段补上。
- 面板 CSS 实际落在 `styles.css` 的 pane 家族区（`.pane-models`），非 design 原写的 components.css——按组件层规范归位。
- i18n 死键清理范围扩大：`mcNoProviders` 之外还有 `mcImportPi/mcImportConfirm/mcImportResult`（值含"从 pi 导入"）、`edTabEnv`（Tab 栏移除后孤儿）。
- ModelsPage 文件头注释含 "import from pi" 字样会挂 AC2 的 grep——一并重写。
- 验证：puppeteer（chrome-headless-shell，本机临时装）跑通 AC1/AC3/AC4/AC5 动态路径；测试沙箱 modeltest（复用 82120d943385 镜像缓存）已删除（含卷）。
