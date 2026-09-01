# 模型配置页按设计稿重设计(open-design Kumo)

## Goal

把 `web/src/panes/models/` 的 React 模型配置页按设计稿
`docs/open-design/screens_model-config.html`（Cloudflare Kumo 单页设计）重设计：
在保留全部现有功能的前提下，让实际页面（golden-layout 内的 `page` pane）的视觉与
交互结构对齐设计稿。设计稿是整页，实际是 pane，因此按用户确认的适配决定落地
（见「适配决定」），不做 1:1 像素搬运。

## 适配决定（用户已确认）

1. **不复刻** crumb 面包屑栏与 中文/EN 语言切换——全局已有 Sidebar/Statusbar/pane 页签栏。
2. **抽屉 = pane 内 scrim + absolute 抽屉**——遮罩只盖 `.pane-models`，不打断其他分屏 pane。

## Requirements

- R1 主 Tab：圆角分段容器（`.ml-tabs`）内凹 + 胶囊选中，视觉对齐设计 `.tabs-primary/.tab-item`。
- R2 供应商页：新增 `.sec-head`（标题 + 副标题 + 右侧「从 pi 导入」「新增供应商」），
  替换现有按钮工具栏；空态保留双按钮。
- R3 供应商卡片：协议徽章独占一行（名称在其下）、hover 浮现工具、mono URL、
  meta（模型数 · 脱敏 key）、chips 顶部分隔线、hover 底色微调；
  grid `minmax(300px,1fr)`。脱敏 key 为空显示 `—`。
- R4 抽屉：pane 内 `.ml-scrim` 淡入 + absolute 抽屉 translateX 滑入；Esc/scrim 点击关闭；
  分组小节标题（`.ml-dgroup`）；高级设置可折叠（chevron 旋转）；
  savebar = 左 btn-danger-text「删除」+ 右「取消」「保存」。
- R5 模型行：折叠态 = chevron + 可编辑 mono id + 名称 + 推理 info 徽章 + 成本 `$i/$o` +
  `.test-pill`（play/spin/ok·ms/fail）+ 删除；展开态 = 显示名+协议覆盖两列、
  ctx/maxOut/推理勾选三列、in/out/cacheR/cacheW 四列、models.dev 填充。
- R6 Discover 弹窗：head（标题 + mono endpoint）、搜索框带 icon、行（checkbox+id+名称+已存在）、
  foot（全选/清空 + `N 已选` + 取消 + 添加所选）。
- R7 增量 agent 页（pi/opencode）：agent 名称(2xl) + 已安装/一致徽章 + `.paradigm-strip`
  说明条；live 回读/分配/原生配置改为 `.form-card` 卡片；`.ml-savebar` 圆角保存条。
- R8 切换式 agent 页（claude/codex）：agent 名称 + paradigm 条 + `.sec-head`（预设标题+新增）；
  预设卡 current 用 accent 1.5px 描边；`provider → model` meta；动作 = 设为当前(primary-sm)
  + 编辑/复制 icon + 删除 danger icon；无 current 显示 `.warn-strip`。
- R9 用量页：工具栏 = 标题 + `.ml-window-switch` 胶囊分段 + 刷新 icon-btn + gen-at；
  图表区固定 `1.4fr 1fr` 两栏（条形+环形，窄屏单栏）；统计卡/明细表保持现有。
- R10 全部新增文案走 i18n（zh/en 双份，见 design.md i18n 表）。

## Acceptance Criteria

> 状态：A1 已验证；A2–A10 已实现并经截图审阅（web/shots/），用户确认存在少量视觉细节偏差，
> 留待后续修复（本次提交不含该部分）。

- [x] A1 `npm run build`（或 `tsc --noEmit && vite build`）通过，无 TS 错误。
- [ ] A2 供应商页：标题「供应商库」+ 副标题 + 右侧两按钮；卡片为徽章上/名下布局，
      meta 显示「N 个模型 · sk-xxxx •••• xxxx」，chips 顶部分隔；hover 浮现编辑/删除。
- [ ] A3 抽屉：点击卡片打开，scrim 淡入且只盖住模型 pane；Esc / scrim 点击 / 取消 均关闭；
      savebar 左删除(danger-text) 右取消+保存；编辑并保存后卡片刷新。
- [ ] A4 模型行：test-pill 三态（检测中 spin / 成功 `✓ {ms}ms` / 失败 `✕`）；展开后
      显示名+协议覆盖两列、推理勾选内联；成本 in/out/cacheR/cacheW 四列。
- [ ] A5 Discover：搜索过滤、全选/清空、`N 已选`、取消关闭、添加所选合并去重。
- [ ] A6 pi/opencode：大号 agent 名 + 已安装/一致徽章 + paradigm 条 + form-card 分组；
      分配保存/应用、live 同步/编辑/删除、结果面板行为不变。
- [ ] A7 claude/codex：current 预设卡 accent 描边；设为当前/编辑/复制/删除可用；
      无 current 显示警告条；保存/应用行为不变。
- [ ] A8 用量页：标题 + 胶囊窗口切换 + icon 刷新；7天默认；统计卡/图表/明细表渲染正确。
- [ ] A9 暗色模式（data-mode=dark）下所有改动区域对比度/配色正确，无硬编码亮色残留。
- [ ] A10 窄 pane（分屏 <520px）不溢出：卡片单列、抽屉贴合、表格横向滚动。

## Out of Scope

- 不改后端 `/api/models/*` 契约（含脱敏 key 格式）。
- 不新增/删除功能（live sync/edit/delete、catalog fill、discover、preset CRUD、用量聚合等全保留）。
- 不迁入设计稿整页的 crumb/lang-toggle（已确认不复刻）。
- 不引入新依赖、不改 golden-layout 外壳。

## Technical Notes

- 样式集中在 `web/src/styles.css` models 区块，沿用 `ml-*` 类名（设计稿裸类映射，见 design.md）。
- Token 层与设计稿同源，直接复用；charts.tsx 的 Kumo 分类调色板保持不变。
- 组件结构不动（ProviderGrid/ProviderEditor/ModelRow/AgentTabs/PresetList/UsageTab/ModelsPane），
  只改 render 结构/className/少量本地 state。
