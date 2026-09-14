# 技术设计 · mgr-web 原型重构

## 1. 边界与总原则

- **只动表现层**：后端契约（api.ts / types.ts payload）、数据流、golden-layout
  接线、gl-kumo.css `.lm_*` 主题机制全部不动。
- **token 一字不差**：styles.css 的 `:root` / `[data-mode="dark"]` 块保持
  原样（这是原型 index.html「设计决策」的明确承诺），只补 mgr-web.css 新增
  的 3 个容器 token（`--container-max` / `--section-y-*` / `--container-gutter-*`），
  其余不变。
- **mgr-web.css / mgr-shell.js 已由设计师补齐**（docs/Web-Prototype/，
  424 行 css + 118 行 js）：组件层与 shell 是**权威源**，实施采用「源码等价
  移植」而非反推：
  - `mgr-web.css`：token 块丢弃（styles.css 已有一致版本，只补新 token）；
    **组件层正文（base/rail/panel/sdot/badge/btn/segmented/tabs/chip/
    表单/card/overlay/dialog/menu/spinner/notice/table/tree/sbx）整段移植**
    进 `mgr-web/src/components.css`，由 main.tsx import。唯一改动：第 6 行
    `@import url(https://fonts.googleapis.com/css2?family=Inter...)` 删除，
    保留项目既有 @fontsource/inter（离线，无外网字体依赖）。
  - `mgr-shell.js`：职责拆成 React 等价物——图标 sprite（55 个 icon 定义）
    移植进 `icons.tsx` 的 PATHS；rail NAV 数组（5 项 grid/terminal/layers/
    sliders/chart）移植进 `App.tsx`；主题切换逻辑沿用现有 `data-mode` +
    `mgr.theme`（原型 js 与现有逻辑一致，直接对齐）；通用 `.menu` 外点/
    Esc 关闭逻辑落进 React 事件层。**不引 `window.icon` 全局**。

## 2. 组件层 CSS（移植 mgr-web.css → components.css）

原型的 `mgr-web.css` 已给出**完整权威组件层**（424 行），实施直接移植进
`mgr-web/src/components.css`（由 main.tsx 在 styles.css 之后 import）。
逐块清单（块名 = mgr-web.css 章节）：

- `base`：box-sizing / body / headings / .icon(.sm/.xs) / .muted/.txs/.tsm /
  .num / .spin / @keyframes rot — 与 styles.css 现有 base 重叠，取并集
  （styles.css 已有的按钮/输入 focus 等保留；缺的 .icon.sm/.xs/.spin 补上）。
- `app shell`：`.app`（flex 100vh）/ `.rail`（48px）/ `.rail-brand` /
  `.rail-nav` / `.rail-foot` / `.rail-btn`（+ active::before 2px accent 指示条、
  `[data-tip]::after` tooltip）/ `.rail-sep` / `.panel`（272px，`app.panel-hidden
  .panel{display:none}`）/ `.panel-head/.panel-title/.panel-count/.panel-body/
  .panel-foot` / `.main`（+ .scroll）/ `.page`（1200px 容器）/ `.page-head`
  （h1 .text-2xl + .sub）/ `.page-actions` / `.statusbar`（26px + .seg）。
- `状态`：`.sdot`（+.stopped/.error/.warn/.info）/ `.badge`（+.ok/.warn/
  .danger/.info/.neutral，均带文字）。
- `按钮`：`.btn`（+ -primary/-secondary/-ghost/-danger-text/-danger/-sm，
  :disabled）/ `.icon-btn`（+ .danger/.lg/:disabled）。
- `分段/页签/芯片`：`.segmented`（button[aria-pressed] + .cnt）/ `.tabs`
  （button[aria-selected]::after 前景下划线 + .cnt）/ `.chip`（+ .off，
  button.chip 可点）。
- `表单`：`.field`（+ label/.field-label/.hint/.err/.invalid）/ `.input`
  （+ select/textarea/.sm/.mono/::placeholder/hover/focus-visible）/ 
  `.input-wrap`（+ .icon/.addon）/ `select.input` 箭头 + `.field-row` /
  `.check`（:checked 前景对勾）/ `.switch`（34×20 前景滑块按钮）/
  `.radio`（:checked 内实心点）。
- `卡片/弹窗/抽屉/菜单`：`.card`（+ .card-pad/.card-head）/ `.overlay`（+
  .open）/ `.dialog`（+ h2/.desc/.actions）/ `.menu`（+.open，button/.icon/
  .danger/:disabled/hr）/ `.spinner` / `.notice`（+ -warn/-info/-danger）。
- `表格`：`.table`（th/td/.r 右对齐 tabular-nums/tr:hover）。
- `沙箱树`：`.tree-node(+open/stopped/is-focus)/.tree-row/.tree-toggle(.chev)/
  .tree-name/.tree-meta/.tree-acts(.always)/.tree-kids/.leaf(.del/.leaf-tag/
  .kbd)/.leaf-sep/.tree-hint/.tree-group`。
- `页面专属（不进 components.css，放页面文件或页级 css）`：sandbox-list 的
  .sbx-grid/.sbx-*/.facts/.line/.svc 与 create 的 .wz/.sec/.side/.steps/.sum
  等页面级类——实施时从对应 HTML 的 `<style>` 提取进对应页面组件；模型页的
  .profile-bar/.pv-grid/.drawer/.preset/.model-opt/.sbx-tbl 等进 models 组件。

**主题要点不变**（design.md §2 末段）：所有类只消费 token 与 color-mix，
无硬编码色；深浅色自动跟 [data-mode]。

**明确不移植**：`.gl-*` 块（css 第 393-425 行，注释="镜像 gl-kumo.css 的
几何"）——静态原型展示用，React 走真实 golden-layout + gl-kumo.css 的
`.lm_*`；`@import url(googleapis Inter)`（离线替代为 @fontsource）。

## 3. Shell 重构（App.tsx + icons.tsx）

现状（App.tsx）：`<aside class="sidebar">` 216px + `mgr.sidebarCollapsed`
（S5/R1，48px 图标栏态）+ `<main class="main">`。

目标（原型）：48px rail + 工作区侧面板 + 1200px 容器。结构对照
**mgr-shell.js 的 NAV**（5 项，图标已见原型 sprite）：

```
<div class="app">
  <IconSprite/>
  <nav class="rail" aria-label="主导航">
    <a class="rail-brand" title="Sandbox 管理器 · 总览"><Icon name="cube"/></a>
    <div class="rail-nav">
      <button class="rail-btn active" data-tip="工作区"><Icon name="grid"/></button>
      <button class="rail-btn"><Icon name="terminal"/></button>   ← 沙箱列表
      <button class="rail-btn"><Icon name="layers"/></button>     ← 镜像
      <button class="rail-btn"><Icon name="sliders"/></button>    ← 模型配置
      <button class="rail-btn"><Icon name="chart"/></button>      ← 用量
    </div>
    <div class="rail-foot">
      <button class="rail-btn"><Icon name="sun|moon"/></button>   ← 主题
      <button class="rail-btn"><Icon name="globe"/></button>      ← 语言
    </div>
  </nav>
  {page === "workspace" && !panelHidden && (
    <aside class="panel">…沙箱树…</aside>
  )}
  <main class="main"><div class="page">…各页…</div></main>
</div>
```

- **rail 图标栏**：48px，垂直排布。导航 5 项对应现有 Page
  （workspace/sandboxes/images/models/usage），激活项 `.rail-btn.active` +
  左侧 2px accent 指示条 + `data-tip` tooltip。**图标名以 mgr-shell.js
  sprite 为准**：grid/terminal/**layers**（镜像是新图标，现有是 box）/
  sliders/chart。镜像图标按原型用 layers，需要补进 icons.tsx。
- **panel**（仅工作区）：`app.panel-hidden` 类 + `mgr.panelHidden` 持久化；
  图标栏「工作区」点击 toggle panel（与原型 workspace.html wsNav 行为一致，
  但注意：现有 App.tsx 的 workspace 导航要同时承担"切页"与"折叠面板"两个
  动作——原型里 workspace 按钮已经是当前页时不切页只折叠，实施对齐该语义）；
  panel 自身的「收起」按钮（panel-head）toggle 同一状态。
- **1200px 容器**：`.page { max-width: var(--container-max); margin: 0 auto;
  }`；工作区（WorkspacePage）主区全宽不受 1200 约束（golden-layout 多窗格
  需要）。`.main.scroll` 管理页滚动；工作区的 main 不 scroll（golden-layout
  自持滚动 + 底部 statusbar）。
- **旧键迁移**：`mgr.sidebarCollapsed` 不再读；`mgr.panelHidden` 新键
  （默认展开）。S5/R1 的侧栏折叠功能被 rail+panel 取代（D5）。
- **popout child**：`IS_POPOUT_CHILD` 分支保持「lone workspace 无 shell」
  （原型 popout 子窗口不带 rail），不受影响。

## 4. 工作区页（WorkspacePage.tsx + SandboxTree.tsx）

- 渲染树保持：panel（沙箱树侧面板）+ 主区 golden-layout + 状态栏。
  golden-layout 接线（glRef/componentState/seq pool/popout/布局持久化
  `mgr.layout`）**逐字保留**（C3）。
- SandboxTree：保留现有 manifest 懒加载 + 服务按钮注册 + S5 flyout 折叠
  （`mgr.treeCollapsed`）；**新增**原型的面板头（刷新/收起）、筛选输入框
  （`tree-filter`，现有无搜索）、节点行结构调整（tree-toggle + tree-meta
  状态文字 + tree-acts 启停/更多）、空提示（停止沙箱显示"未运行"）。
- 注册自定义按钮对话框（RegisterDialog）与新「更多」菜单：现有
  RegisterDialog.tsx（.dialog 复刻原型 Type 单选/字段校验/端口探测）；
  「更多」菜单（.menu，固定定位）是现有所缺，新增一个 `NodeMenu` 组件
  （terminal/enter/start/stop/restart/edit/register，停用态 disabled）。
- 空状态（.gl-empty）+ 状态栏（.statusbar：已连接/沙箱计数/窗格计数/布局
  保存时间）按原型还原。
- 原型 .term/.frame/.cs-card 视觉：现有 XtermPane/IframePane/CodeServerPane
  的持有态是真实功能（xterm 实例、iframe、按需拉起三态）。**只映射
  CodeServerPane 的"启动中"卡与 IframePane 的地址条**到新组件类，不动
  xterm/iframe 行为。

## 5. 沙箱列表页（SandboxListPage.tsx）

现状已有：4s 轮询、启停/重启、删除确认（volumes 勾选）、profile 指派 chip
（09-10 S2 落地）、AdoptPage 入口、JobView 跳转。改造：

- **分段筛选**：`.segmented`（全部/运行中/已停止/外部栈 + 计数）。现有
  types.ts Sandbox 有 `live`/`adopted` 字段可判断状态；外部栈 = adopted。
- **搜索**：`.input-wrap + .input.sm`，前端过滤 `name.includes(q)`。
- **卡片重排**：按原型 .sbx-head（状态点+名称+badge+进入按钮）→ .sbx-body
  （入口 URL mono / facts 网格 镜像·资源·创建·profile / 已安装服务 chips /
  容器行 svc）→ .sbx-foot（启动/停止/重启/编辑/删除）。
- **快速指派 popover**：现有 AgentAssignControl.tsx（.pop）已有 profile+
  agent 子集指派逻辑，升格为卡片里 profile chip 点击弹出的 popover（原型
  .pop：profile select + agent checkboxes + 保存），用 `putSandboxModelProfile`
  契约（09-10 S2 已实现，本任务 prd D7）。
- **删除 alertdialog**：现状已有，换原型 `.dialog` 样式 + 外部栈换「移除
  登记」文案（现有已按 adopted 分支，只换视觉）。

## 6. 新建向导（CreatePage.tsx + EnvPicker + ServicesPicker）

现状：CreatePage 单页（名称/服务开关/场景/资源 顺序经 EnvPicker/
ServicesPicker 组合）+ 提交进 JobView。改造：
- 重组为原型四段（.sec + 编号圆圈 .n）+ **右侧粘性摘要 .aside.side**
  （步骤 nav + 配置摘要卡片：入口/服务 chips/场景版本/资源/镜像复用判断）。
- 服务开关：现有 ServicesPicker 已是四开关含 pi-web 依赖联动，保留逻辑、
  套 .row + .switch 样式。
- 场景选择：现有 EnvPicker 已按 L1-L4 分组（09-10 batch2 D2 落地），套
  .layer + .row + .check 样式；必装锁定（node/python）用 disabled 复选 +
  .lock 标签。
- 右侧摘要数据流：CreatePage 本地 state 汇总（name/svc/scn/ver/cpu/mem）
  → 摘要组件实时渲染 + 按钮禁用逻辑；镜像复用判断用现有 envhash 相关
  API/逻辑（后端 `GET /api/images?filter=usable` 或创建接口已有的判断；
  实施以现状为准，不新造后端）。
- 步骤导航：IntersectionObserver 高亮当前段（原型 create-sandbox.html 的
  实现直接借鉴）+ 锚点 click 滚动到对应 .sec。
- AdoptPage/EditPage 共用 EnvPicker/ServicesPicker，改样式即自动统一。

## 7. 模型配置页（pages/models/*，最大改造面）

现状：ModelsPage（profile 选择栏 + 五 tab）+ ProviderGrid/ProviderEditor/
AgentTabs/PresetList/ModelRow/ModelPicker/MgrNotice，共 ~1034 行。改造
**组件结构换新形态、API 契约不变**：

- **profile 条**：.profile-bar + .segmented（profile 计数）+ 重命名/删除/
  新建按钮 + 未指派提示。现有 profile 选择逻辑复用。
- **供应商 tab**：.pv-grid 供应商卡片（名称+协议 badge+baseUrl+模型
  chips+使用处/密钥状态）+「新增供应商」卡；编辑抽屉 .drawer
  （名称/协议/baseUrl/API key 显隐/模型列表编辑/测试连接/保存）。
  ProviderEditor 现有功能与字段全保留，换成 .drawer 形态。
- **pi / opencode tab**：.strip 说明 + .two 双栏（左 .assign 当前指向
  单选组 .model-opt，右 .sbx-tbl 生效沙箱表带 .switch 勾选）。
- **claude / codex tab**：.preset 卡片列表（当前高亮/切换/复制/编辑/删除
  /新建）+ .sbx-tbl 生效沙箱。
- ProviderGrid/AgentTabs/PresetList 的**现有数据 fetch 与保存逻辑**迁移到
  新形态组件（可合并/拆分），不改 `?profile=` 与 PUT 语义。
- MgrNotice（后端提示）保留位置。

## 8. 其余 5 页（JobView/Images/Usage/Adopt/Edit）

- 全部套用：rail shell（自动）+ .page 容器 + .page-head 页头（h1+sub+
  actions）+ 现有表格/卡片加组件类。
- JobView：保持轮询与日志视图，换 shell/页头/statusbar 样式。
- ImagesPage：保持表格与删除/清理，套 .table/.btn/.badge。
- UsagePage：保持图表（charts.tsx），套 .segmented/.chip 筛选 + 页头。
- 不做结构性改造（除每页已存在的功能），只保证视觉一致（AC7）。

## 9. 主题与无障碍

- 主题：App.tsx 现有 `data-mode` + `mgr.theme` 持久化不变；rail 底部
  主题/语言按钮沿用现有逻辑。
- 无障碍：新增弹层带 role/aria-label（原型已带）；对话框 Escape 关闭、
  焦点管理（现有 RegisterDialog 已有，popover/menu/drawer 新增时沿用同
  模式）；`aria-pressed`/`aria-expanded` 沿用现有用法。

## 10. 风险与回滚

- **风险1 现 styles.css 组件层与移植的 components.css 重叠/冲突**：现有
  styles.css 已有 .btn/.badge/.overlay/.dialog/.field/.sbx-grid/.statusbar/
  .dot 等组件类（与 mgr-web.css 同名不同形）。**实施时以 components.css
  （设计师版）为准覆盖**，styles.css 中的旧组件类删除或改由 components.css
  接管，避免双源。逐一核对每个重名类（.btn 族/.badge/.field/.input/
  .overlay/.dialog/.statusbar/.sbx-grid 等），按设计师版迁移。
- **风险2 模型页大改引入回归**：分步实施（先供应商 tab 新形态，再 agent
  tabs），每步可独立验证；API 契约不变所以可随时切回旧 UI。
- **风险3 原型静态页 vs React 实渲视觉偏差**：以 mgr.localhost 实渲对照
  原型静态页截图核销；.gl-* 走真实 golden-layout 是其最大差异（预期）。
- **回滚**：纯前端改动，git revert 即回滚；功能层未动，风险低。

## 11. 交付物清单（改动文件）

- `mgr-web/src/components.css`（新增，移植 mgr-web.css 组件层；main.tsx import）
- `mgr-web/src/App.tsx`（rail shell：NAV 5 项 + panel + mgr.panelHidden）
- `mgr-web/src/icons.tsx`（补 mgr-shell.js 新增图标：layers/arrowr/panel/
  more/info/key/link/external/desktop/popout/maximise 等；合并保留现有 33 个）
- `mgr-web/src/styles.css`（token 块补 3 个容器 token；删除被 components.css
  接管的旧组件类如 .sidebar/.sb-list/.launch-btn/.dot 等，逐一核对）
- `mgr-web/src/pages/WorkspacePage.tsx`（面板 + 空状态 + 状态栏）
- `mgr-web/src/pages/workspace/SandboxTree.tsx`（面板头/筛选/节点结构）
- `mgr-web/src/pages/workspace/NodeMenu.tsx`（新增 更多菜单）
- `mgr-web/src/pages/SandboxListPage.tsx`（筛选/搜索/卡片/指派 popover）
- `mgr-web/src/pages/CreatePage.tsx`（四段 + 右侧摘要）、EnvPicker、
  ServicesPicker（样式型）
- `mgr-web/src/pages/models/*`（新形态，最大面）
- `mgr-web/src/pages/{JobView,ImagesPage,UsagePage,AdoptPage,EditPage}.tsx`
  （套 shell/组件）
- `mgr-web/src/i18n.ts`（双语新增文案）
- `docs/Web-Prototype/`（原型目录本次只读；如需可加 README 说明对应关系）