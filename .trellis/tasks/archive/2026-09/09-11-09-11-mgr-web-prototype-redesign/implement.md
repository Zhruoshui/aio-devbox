# 执行计划 · mgr-web 原型重构

## 阶段总览

分 7 个可独立验证的步骤，从底层组件层往上走，每步构建通过 + 可目视。
后端 / API / golden-layout 接线全不动，纯前端。

```
S0 基线勘察（现有各页现状快照 + 原型逐类提取）
S1 组件层 CSS + 图标 + i18n 骨架（components.css 合并原型全部类）
S2 Shell 重构（rail 图标栏 + 工作区侧面板 + 1200px 容器 + 键迁移）
S3 工作区页（面板头/筛选/节点结构/更多菜单/空状态/状态栏）
S4 沙箱列表页（分段筛选/搜索/卡片重排/指派 popover/删除 dialog）
S5 新建沙箱页（四段 + 右侧粘性摘要 + 步骤导航 + 服务/场景联动）
S6 模型配置页（profile 条 + 供应商卡片/抽屉 + agent 指派新形态）
S7 其余 5 页套壳 + 全站构建/类型/交互验收 + spec 更新 + 提交
```

## S0 基线勘察回顾（已完成）

- 原型 7 文件全部读完（5 HTML + mgr-web.css 424 行 + mgr-shell.js 118 行）。
- **mgr-web.css / mgr-shell.js 已由设计师补齐**（非反推，直接移植）。
- 现有源码核查：styles.css（token+组件）、gl-kumo.css、App.tsx、icons.tsx、
  i18n.ts（双语 705 行）、WorkspacePage/SandboxTree、SandboxListPage、
  ModelsPage 结构、frontend spec（component/state 两份）。
- 结论已落 prd.md D1-D8 / design.md。

## S1 组件层 CSS + 图标 + i18n 骨架

**步骤**：
1. 新建 `mgr-web/src/components.css`，**移植 mgr-web.css 的组件层正文**
   （base/rail/panel/sdot/badge/btn/segmented/tabs/chip/表单/card/overlay/
   dialog/menu/spinner/notice/table/tree 各块；design.md §2 精确清单）。
   只消费 token，无硬编码颜色；删除 `@import url(googleapis Inter)`（用
   项目既有 @fontsource/inter）。components.css 由 `main.tsx` import
   （在 styles.css 之后）。
2. 与现有 styles.css 组件类**逐一核对重名覆盖**（risk1）：.btn 族/.badge/
   .field/.input/.overlay/.dialog/.statusbar/.sbx-grid 等以设计师版为准，
   删除 styles.css 中被接管的旧类（.sidebar/.sb-list/.launch-btn/.dot 等）。
3. `styles.css` token 块补 3 个容器 token（`--container-max` /
   `--section-y-*` / `--container-gutter-*`）；其余 token 不动。
4. `icons.tsx` 补 mgr-shell.js 新增图标：layers/arrowr/panel/more/info/
   key/link/external/desktop/popout/maximise 等（stroke 一致，viewBox 24，
   进 PATHS 与 IconName；现有 33 个保留，重复的以 mgr-shell.js 为准）。
5. `i18n.ts` 增补本轮全部新文案（rail 语义、筛选、"更多"菜单、抽屉、
   摘要、指派 popover 等），zh-CN/en 双列。

**完成门**：`cd mgr-web && npm run build` 通过；无新 type 错误。

## S2 Shell 重构（App.tsx）

1. `<aside.sidebar>` → `<nav.rail>`：品牌 cube + 五导航图标按钮
   （grid/terminal/**layers**/sliders/chart，激活 2px accent 指示 + data-tip
   tooltip）+ 底部主题/语言。导航点击仍走 `nav(page)`（镜像图标用
   mgr-shell.js 的 layers）。
2. 工作区侧面板：`page === "workspace" && !panelHidden` 时渲染
   `<aside.panel>`（沙箱树容器）。默认展开。
3. 持久化：`mgr.panelHidden`（"1"折叠），读写在 App；图标栏「工作区」
   按钮与面板头「收起」按钮 toggle 同一状态。
4. `mgr.sidebarCollapsed` 停止读写（prd D5 迁移）。
5. `.page` 容器：管理页内容包 `.page`（max-width 1200px 居中）；
   WorkspacePage 主区全宽（golden-layout 需要）。
6. keep: IS_POPOUT_CHILD lone workspace（无 shell）。

**完成门**：build 通过；mgr.localhost 全站 rail 生效，工作区有面板、管理页
无面板，主题/语言切换正常，折叠加刷新保持。

## S3 工作区页（WorkspacePage + SandboxTree + NodeMenu）

1. SandboxTree：面板头（标题+计数+刷新/收起 icon-btn）+ `.ws-search` 筛选
   input + 节点行新结构（tree-toggle: 展开箭头+状态点+名称+tree-meta 状态
   文字；tree-acts: 停止态启动 / 更多按钮）+ 服务叶子（含"注册自定义按钮"
   行）+ 停止沙箱"未运行"提示。保留 manifest 懒加载与 S5 flyout 折叠
   （`mgr.treeCollapsed` 独立键不变）。
2. 新增 `NodeMenu.tsx`：`更多` fixed-position `.menu`（打开终端/列表中查看/
   启动/停止/重启/编辑配置/注册自定义按钮，停用态 disabled）。动作沿用
   sandboxAction / onManage / onRegister 现有回调。
3. 空状态 `.gl-empty`（无窗格时）+ 状态栏 `.statusbar`（已连接 mgr-api /
   沙箱计数 / 窗格计数 / 布局保存时间）。计数逻辑从原型 workspace.html 的
   renderTree 借鉴（现有 st-sb/st-panes 内容替换成新 statusbar）。
4. 注册对话框 RegisterDialog：换 `.dialog`/`.field` 样式（现有逻辑含
   type segmented + 端口探测已具备，只换视觉类；原型 .type-row/.type-opt
   的 radio 单选形态从 models.html 同款迁移）。

**完成门**：build 通过；工作区面板树可展开/筛选/启停/更多菜单/注册按钮，
空状态与状态栏正确，golden-layout 窗格打开/关闭/拖拽不受影响。

**完成门**：build 通过；工作区面板树可展开/筛选/启停/更多菜单/注册按钮，
空状态与状态栏正确，golden-layout 窗格打开/关闭/拖拽不受影响。

## S4 沙箱列表页（SandboxListPage）

1. `.segmented` 分段筛选（全部/运行中/已停止/外部栈 + 计数）：`live` +
   `adopted` 判断状态（外部栈 = adopted）。
2. 名称搜索 `.input.sm`，前端 `name.includes(q)` 过滤。
3. 卡片按原型重排：`.sbx-head`（状态点+名称+badge+进入按钮）→ `.sbx-body`
   （sbx-url 入口 mono / facts 网格 镜像·资源·创建·profile chip / 已安装
   服务 chips / 容器行 svc）→ `.sbx-foot`（启动/停止/重启/编辑/删除）。
   进入按钮走 `onEnter`；启停走现有 `act()`。
4. 快速指派 popover：复用 AgentAssignControl 逻辑，改为 profile chip 点击
   弹出的 `.pop`（profile select + agent checkboxes + 保存，
   `putSandboxModelProfile` 契约，prd D7）；Escape/外点关闭。
5. 删除 alertdialog：现有逻辑换 `.dialog` 样式；外部栈「移除登记」文案与
   隐藏 volumes 行（现状已按 adopted 分支，只换视觉）。

**完成门**：build 通过；筛选/搜索/启停/指派/删除（原生+外部栈）全链路可用，
卡片信息层级与原型一致。

## S5 新建沙箱页（CreatePage + EnvPicker + ServicesPicker）

1. CreatePage 重组为四段 `.sec`（编号圆 .n）：名称→服务→场景→资源，右侧
   `.aside.side`（步骤 nav .steps + 摘要 .sum）。
2. 摘要实时渲染：入口子域名（名称前缀）、服务 chips、场景与版本、资源、
   镜像「复用现有镜像 / 新组合需构建」（沿用现有 envhash/镜像判断逻辑，
   不新造后端）。
3. 服务开关 ServicesPicker：改 `.row + .switch` 样式；保持四开关与 pi-web
   依赖联动（现有逻辑）。
4. 场景 EnvPicker：改 `.layer + .row + .check` 样式；L1-L4 分组、必装锁定
   （node/python disabled + .lock）保持现有 data（category/always_on）。
5. 步骤导航 IntersectionObserver 高亮（借鉴 create-sandbox.html 实现）+
   锚点 scrollTo；名称/CPU/内存校验沿用现有（含后端 400 兜底描述）。
6. 提交 → JobView 不变。

**完成门**：build 通过；四段向导 + 右侧摘要联动正确，服务依赖、镜像复用
判断、名称/资源校验全可用，提交进 JobView。

## S6 模型配置页（pages/models/*）

1. ModelsPage：`.profile-bar`（.segmented profile 计数 + 重命名/删除/新建
   按钮 + 未指派 hint）→ `.tabs` 五 tab（供应商库/pi/opencode/Claude
   Code/Codex）→ `.tab-panel`。
2. 供应商 tab：`.pv-grid` 卡片 + `新增供应商` 卡；`.drawer` 编辑抽屉
   （名称/协议/baseUrl/API key 显隐/模型列表编辑/发现模型/测试连接/保存）
   ——ProviderEditor 功能迁移到 drawer 形态，`?profile=` 与 PUT 语义不变。
3. pi/opencode tab：`.strip` 说明 + `.two` 双栏（左 `.assign` 单选组
   `.model-opt`，右 `.sbx-tbl` 生效沙箱 .switch 勾选 + 属于其他 profile /
   未指派状态行）。
4. claude/codex tab：`.preset` 卡片列表（当前高亮/切换/复制/编辑/删除/
   新建）+ `.sbx-tbl` 生效沙箱。
5. ProviderGrid/AgentTabs/PresetList 现有 fetch/save 逻辑迁移进新形态
   （数据契约不变）；MgrNotice 保留。
6. 增量保存语义（dirty set / 放弃 / 保存）沿用现有。

**完成门**：build 通过；per-profile 各 tab 数据正确读写，抽屉/预设/生效沙箱
交互可用，无 API 契约变化。

## S7 其余 5 页套壳 + 验收 + spec 更新 + 提交

1. JobView / ImagesPage / UsagePage / AdoptPage / EditPage 套 `.page` 容器 +
   `.page-head` 页头 + 现有表格/卡片换组件类；功能与数据不动。
2. **原型文件入库**：`docs/Web-Prototype/` 7 个文件在本次提交中一并 `git
   add`（当前 untracked；作为设计基准保留在仓库，含 mgr-web.css/mgr-shell.js）。
3. 全站验证：
   - `cd mgr-web && npm run build`（tsc + vite）通过。
   - `make mgr-up` 起 mgr 栈，mgr.localhost 逐页交互：工作区（开终端/编辑器/
     iframe/code-server、面板折叠、更多菜单、注册按钮模板化重启、popout）、
     列表（筛选/搜索/启停/指派/删除）、创建向导、模型配置、JobView/镜像/
     用量/导入/编辑。
   - 浅深色切换全站检查（token 覆盖一致性，无硬编码色残留）。
   - 对照原型静态页做视觉 diff（关键截图：workspace / sandbox-list /
     create-sandbox / models），偏差清单核销。
   - 双语切换检查（zh-CN/en 无英文漏网 / 无中文残留）。
4. spec 更新：`.trellis/spec/frontend/*`（index.md 补 rail shell 说明、
   directory-structure.md 补 components.css / NodeMenu / 新 models 结构；
   component-guidelines 若新增弹层组件补充约定）。
5. 提交：`git add` 相关文件 + `git commit`（feat(mgr): 按 Web-Prototype
   原型重构全站 UI，rail+panel shell 与 4 页重设计）。

## 验证命令速查

```bash
cd mgr-web && npm run build       # 构建门（= tsc --noEmit && vite build）
cd mgr-web && npm run dev         # 本地 dev preview（无后端数据，看布局）
make mgr-up                       # 完整控制面 mgr.localhost（需端口发布）
```

## 检查门（每步结束）

- [ ] `npm run build` 通过（typecheck 是构建门）
- [ ] 该步涉及的页面在 mgr.localhost 或 dev server 可交互
- [ ] 无组件级手动深色变体（只走 token/color-mix）
- [ ] 新增文案双语齐全（i18n.ts）
- [ ] 新弹层有 role/aria，Escape 可关
- [ ] `git diff` 无后端 / api.ts / types.ts 契约改动（除新增图标类型）