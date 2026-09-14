# 按 docs/Web-Prototype 高保真原型重构 mgr-web 全站 UI

## Goal

以设计师交付的高保真原型 `docs/Web-Prototype/`（5 个 HTML：index / workspace /
sandbox-list / create-sandbox / models）为准，重构 mgr-web 全站界面。原型
index.html 自身定义了本轮范围：**4 个页面重设计**（工作区 · 沙箱列表 · 新建
向导 · 模型配置），**5 个页面沿用现状、视觉套用新组件层**（JobView ·
ImagesPage · UsagePage · AdoptPage · EditPage）。

原型是「在已完成功能之上的布局/视觉重设计」——现有 mgr-web 已实现 09-09
unified + 09-10 UX batch2（服务四开关、场景 L1-L4、模型 per-profile 指派、
侧栏折叠）。本任务**不改变任何现有功能与后端契约**，只改前端表现层。

## 源需求（用户 + 原型，2026-09-11）

1. `docs/Web-Prototype/` 是设计师对**整个系统**的重设计原型，严格遵循并据此
   修改现有系统。
2. 原型 index.html 页脚声明了本轮范围图：已重设计 4 页 / 沿用的 5 页。
3. 原型 index.html「设计决策」两卡：Shell 一条图标栏 + 一个侧面板；Token
   原样沿用（styles.css 的 :root/[data-mode] 一字不差，不另起命名，深浅色只靠
   token 覆盖同步，强调色仅用于主 CTA 与激活指示）。

## 关键约束（原型明确）

- **C1 不另起设计语言**：沿用 Kumo token 层（styles.css 的 `:root` /
  `[data-mode="dark"]` 块一字不差）。新组件一律消费这些 token，禁止在组件
  里写手动深色变体。
- **C2 Shell 形态**：216px 侧栏 → **48px 图标栏（rail）**；侧面板仅在
  工作区出现（沙箱树），其他管理页收起侧面板拿全宽；容器上限 1200px。
- **C3 golden-layout 不动**：40px 标签条、拖拽分屏、弹出子窗口行为保持；
  继续用 gl-kumo.css 主题化 `.lm_*`。原型里的 `.gl-*` 类只是静态镜像，
  **实施时不引入 .gl-\* 类**。
- **C4 状态双表达**：状态用「颜色 + 文字」双重表达（徽标、状态点均带文字），
  不依赖单一颜色。
- **C5 标题风格**：沿用 12/13/14px 紧凑刻度与 4px 节奏；标题句首大写、
  600 字重，不做字距与全大写。
- **C6 双语**：现有 i18n.ts 是 zh-CN/en 双语表（705 行），原型文案全中文——
  新增文案必须进 i18n.ts，双语齐全。页面 `data-od-id` 是原型自测锚点，
  **实施时不必照抄**。

## 已知差距（规划勘察得出）

- **D1 设计师已补齐共享文件**：`mgr-web.css`（424 行）+ `mgr-shell.js`
  （118 行）现已在 `docs/Web-Prototype/`，是**权威**的组件层 CSS 与
  shell 脚本（含完整图标 sprite 55 个 + NAV 导航定义）。实施时**直接移植**
  进 `mgr-web/src/components.css` 与 React 组件（不是从 HTML 反推）。
  注意：原型 css 第 6 行 `@import url(https://fonts.googleapis.com/...)`
  需替换为项目已有的 @fontsource/inter（离线、无外网依赖——CLAUDE.md
  网络策略限制外网字体）。
- **D2 原型 css 的 token 扩展**：mgr-web.css 的 `:root` 与现 styles.css
  的 token **几乎相同**，但新增 `--container-max:1200px` /
  `--section-y-*` / `--container-gutter-*` 三个容器 token。styles.css 需
  补上这三个 token（其余 token 一字不差，遵 C1）。
- **D3 图标缺口**：mgr-shell.js 的 sprite 有 55 个图标；现有 icons.tsx 有
  33 个。需把原型专用/新增的（arrowr/panel/more/info/key/link/external/
  desktop/popout/maximise/layers/cpu/bolt/filter/home/clock/list）等补进
  icons.tsx（stroke 风格、viewBox 24 与现有一致）。
- **D4 `.gl-*` 类不引入 React**：原型 css 的 `.gl-*`（.gl-row/.gl-stack/
  .gl-tab/.gl-ctl 等，css 第 393-425 行）注释明确是"镜像 gl-kumo.css 的
  几何"，只给静态原型展示 golden-layout 视觉。React 实现仍走真实
  golden-layout + gl-kumo.css 的 `.lm_*` 主题，**不引入 .gl-\* 类**（C3）。
- **D5 现有 S5 折叠 vs 原型 rail**：现有 App.tsx 已有 `mgr.sidebarCollapsed`
  （216px ↔ 48px 图标栏）与 SandboxTree 的 `mgr.treeCollapsed`（折叠成竖向
  图标条 + hover flyout）。原型把这两者合并成「rail 图标栏 + 侧面板」一个
  折叠状态（`mgr.panelHidden`）。**迁移策略**：rail 图标栏承载导航 + 主题 +
  语言；侧面板只在工作区出现；折叠状态以 `mgr.panelHidden`
  持久化（与原型一致）；旧 `mgr.sidebarCollapsed` 停止使用。sandbox tree 的
  flyout 折叠是另一维度（tree 自身横向收窄），原型 workspace.html 未重画
  该形态——保留现有 SandboxTree 的 flyout 能力（S5 已验证），仅在
  rail 语义上对齐「收起侧面板」。
- **D6 模型配置是最大改造面**：现有 ModelsPage 1034 行（ProviderGrid /
  ProviderEditor / AgentTabs / PresetList / ModelRow / ModelPicker）。
  原型形态为 .profile-bar + .tabs + .pv-grid 供应商卡片 + .drawer 编辑抽屉 +
  .model-opt 单选组 + .preset 卡片 + 生效沙箱表（.sbx-tbl）。**改组件结构
  而非只换肤**，需重构 pages/models/*（不破坏现有 API 契约）。
- **D7 沙箱列表新增控件**：分段筛选（全部/运行中/已停止/外部栈）+ 名称搜索 +
  快速指派 popover（09-10 S2 已有 putSandboxModelProfile 契约，原型把它
  从卡片编辑器升格为 popover）。现有卡片已有 profile 指派 chip（09-10
  S2 落地），popover 是新增交互。
- **D8 新建向导新增形态**：右侧粘性摘要（入口/服务/场景/资源/镜像复用判断）
  + 步骤导航（IntersectionObserver 高亮）+ 服务联动（pi-web 依赖 pi+vnc）。
  现有 CreatePage 184 行 + EnvPicker（L1-L4 已有）+ ServicesPicker（四开关
  已有）——需按原型重组为单页四段 + 右侧摘要，不是新做逻辑。

## 范围边界

**本轮做**（严格照原型）：
- Shell：rail 图标栏 + 工作区侧面板 + 1200px 容器 + 主题/语言/折叠持久化。
- 工作区页：rail + 面板（沙箱树）+ golden-layout 主区 + 状态栏。
- 沙箱列表页：分段筛选 + 搜索 + 卡片新信息层级 + 快速指派 popover + 删除
  alertdialog。
- 新建沙箱页：四段单页向导 + 右侧粘性摘要 + 服务/场景/资源联动。
- 模型配置页：profile 选择条 + 五 tab + 供应商卡片/抽屉 + agent 指派新形态。
- 缺失的 mgr-web.css / mgr-shell.js 组件层重建（等价物落在
  styles.css + 新组件 css 与 App.tsx / 各页）。
- 其余 5 页（JobView/Images/Usage/Adopt/Edit）套用新 Shell 与页头/卡片/表格
  组件，保持视觉统一（不改功能与数据）。

**本轮不做**：
- 不改任何后端契约 / API / 数据流（api.ts / types.ts 只增组件层所需，不改
  payload 形状与语义）。
- 不动 golden-layout 的 `.lm_*` 主题机制（gl-kumo.css），不引入 `.gl-*` 类。
- 不改沙箱内部应用（code-server / VNC / pi-web 自身 UI，它们在 iframe 里）。
- 不为设计重建沙箱 / 供应商 / 模型假数据进真实 API。

## 验收标准（Acceptance Criteria）

1. **Shell**：全站 216px 侧栏消失，改为 48px 图标栏 + 图标栏导航
   （工作区/沙箱/镜像/模型/用量）+ 主题/语言切换；工作区页有可收起侧面板
   （沙箱树），管理页无侧面板、内容全宽且容器 ≤1200px；折叠状态
   `mgr.panelHidden` 持久化，刷新/翻页后保持。
2. **Token 一致性**：styles.css 的 `:root`/`[data-mode]` 块保持（可微调
   值，不另起命名）；浅深色仅靠 token 覆盖同步；无组件级手动暗色变体。
3. **工作区**：沙箱树侧面板（筛选 + 节点启停 + 更多菜单 + 注册自定义按钮）、
   空状态、状态栏（已连接 mgr-api / 沙箱与窗格计数 / 布局保存） 与原型一致；
   golden-layout 窗格（终端/编辑器/iframe/code-server 按需拉起）行为不变。
4. **沙箱列表**：分段筛选（全部/运行中/已停止/外部栈，含计数）+ 名称搜索 +
   卡片信息层级（入口子域名/镜像/资源/创建/profile chip 快速指派/已安装
   服务/容器状态）+ 删除 alertdialog（外部栈换「移除登记」文案）全部可用。
5. **新建沙箱**：四段单页向导（名称→服务→场景→资源），右侧粘性摘要实时
   反映（入口子域名/服务 chips/场景与版本/资源/镜像「复用或需构建」），
   服务四开关含 pi-web 依赖联动，名称校验/场景层级/资源校验与现有后端
   校验链一致（后端 400 兜底文案保留）。
6. **模型配置**：profile 选择条（新建/重命名/删除/计数）+ 五 tab
   （供应商库/pi/opencode/Claude Code/Codex）+ 供应商卡片/编辑抽屉/发现模型
   + agent 指派（pi/opencode 单选组、claude/codex 预设切换）+ 生效沙箱表
   全部照原型可用；per-profile 数据经 `?profile=` 与现有 API 一致。
7. **一致性**：JobView/ImagesPage/UsagePage/AdoptPage/EditPage 套用新 Shell
   + 页头/卡片/表格组件后视觉统一（功能与数据不变）。
8. **双语**：所有新增文案进 i18n.ts，zh-CN/en 齐全，切换后无英文漏网。
9. **构建门**：`npm run build`（= tsc --noEmit && vite build）通过；无 type
   错误。改动后 `make mgr-up` 正常，mgr.localhost 各页可交互。
10. **无障碍**：新增对话框/抽屉/菜单/弹层有 role/aria（原型已带
    role=dialog/alertdialog/tablist/treeitem 等，实施延续）；键盘可关
    （Escape）；状态不只靠颜色（徽标带文字）。

## 原型与源码锚点（实施勘察起点）

| 原型文件 | 对应源码（现） | 说明 |
|---|---|---|
| mgr-web.css（设计师已补齐） | src/styles.css + 新 components.css | token 块 + 新增 .rail/.panel/.tree-*/.segmented/.switch/.badge/.chip/.drawer/.menu/.pop 组件层，直接移植 |
| mgr-shell.js（设计师已补齐） | src/App.tsx · src/icons.tsx | rail 图标栏 NAV + 主题切换 + 55 图标 sprite；移植成 React，不引 window.icon |
| workspace.html | pages/WorkspacePage.tsx · workspace/SandboxTree.tsx · RegisterDialog.tsx · gl-kumo.css | 侧面板 + 沙箱树 + golden-layout 主区 + 状态栏 + 空状态 |
| sandbox-list.html | pages/SandboxListPage.tsx | 卡片/筛选/搜索/删除 alertdialog/快速指派 popover |
| create-sandbox.html | pages/CreatePage.tsx · EnvPicker.tsx · ServicesPicker.tsx | 四段向导 + 右侧摘要 + 服务联动 + 镜像复用判断（envhash） |
| models.html | pages/models/*（ModelsPage/ProviderGrid/ProviderEditor/AgentTabs/PresetList/ModelRow/ModelPicker/MgrNotice） | profile 条 + 卡片/抽屉 + agent 指派新形态 + 生效沙箱表 |