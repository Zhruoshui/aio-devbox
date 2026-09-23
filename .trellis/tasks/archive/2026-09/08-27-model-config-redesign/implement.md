# 实施 —— 模型配置页按设计稿重设计

顺序：先样式基座，再逐组件替换，最后回归验证。每个组件改动后跑一次 typecheck。

## 验证命令

- Typecheck / 构建门：`cd web && npm run build`（内部含 `tsc --noEmit`）。
  快捷类型检查：`cd web && npx tsc --noEmit`。
- 手动/浏览器验证：`cd web && npm run dev`（Vite dev），浏览器开模型配置 pane；
  或跑一次 `npm run dev` 后截屏核对。暗色切换验证 `data-mode=dark`。
- 回归清单 = prd.md 的 A1–A10。

## 实施清单

> 状态：全部完成（08-27 提交）。已知少量视觉细节偏差（用户审阅截图后确认），后续单独修复。

1. **styles.css —— token/helper 补齐 + 分段 Tab + sec-head + grid/card**
   - `.pane-models` 加 `position:relative`（scrim/drawer 定位基准）。
   - `.ml-tabs`：圆角容器（padding 3px、radius-lg、gap 2px、bg surface-warm、margin-bottom space-6）。
   - `.ml-tab`：高 34px、radius-md、muted→active(surface+elev-ring+fg+500)。
   - 新增 `.ml-sec-head` / `.ml-sec-actions`；`.ml-toolbar` 保留给需要处或删除引用。
   - `.ml-grid` → `minmax(300px,1fr)` + gap space-4。
   - `.ml-card`：hover 底色微调（surface 92%+fg）；`.ml-card-head` 改 column（badge 上/名下）；
     `.ml-card-name` 独立行；`.ml-card-meta` 加 `·` 分隔；`.ml-card-chips` 加 border-top+padding-top；
     新增 `.ml-card-mask`（脱敏 key 样式，mono）。
   - 新增 `.btn-danger-text`（若全局无）。

2. **styles.css —— 抽屉/scrim/dgroup/model-row/test-pill**
   - 新增 `.ml-scrim`（absolute inset0、bg 遮罩、`.open` 淡入）+ 过渡。
   - `.ml-drawer`：absolute top/right/bottom 0、宽 min(460px,94%)、translateX(100%)→`.open` 0；
     移除 keyframe 动画；z-index 分层（scrim < drawer）。
   - 新增 `.ml-dgroup` / `.ml-dgroup-toggle`（chevron 旋转收起）。
   - `.ml-drawer-savebar`：左 danger-text 删除 + spacer + 取消 + 保存。
   - `.ml-model-row`：折叠头 mono id 样式（透明底、focus 显示框）；`.ml-badge-info`（accent 底+dot）；
     成本 `$i/$o` mono；展开体 field-row / field-row-3 / field-row-4 布局。
   - 新增 `.ml-test-pill`（24px 胶囊 + spin + ok/fail 态）。

3. **styles.css —— discover 弹窗 / agent 页 / preset / usage**
   - `.ml-discover`：head 加 h2+endpoint mono、搜索框加 icon、foot 加取消 + 已选计数。
   - 新增 `.ml-agent-head`（2xl 名）、`.ml-paradigm-strip`、`.ml-form-card`、`.ml-savebar`、
     `.ml-native-row`。
   - `.ml-preset-card` current 描边（accent 1.5px）；`.ml-warn-strip`。
   - `.ml-usage-toolbar` + `.ml-window-switch`；`.ml-charts` 改固定 `1.4fr 1fr`（<900px 单栏）；
     `.ml-bar-row` 对齐 label 130px + val 74px。

4. **i18n.ts** —— 加 design.md i18n 表的新 key（zh/en）。

5. **ModelsPane.tsx** —— providers 分支：`.ml-toolbar` → `.ml-sec-head`（标题/副标题/两按钮）；
   空态标题+副标题+双按钮；用量分支传新 props（如需要）。

6. **ProviderGrid.tsx** —— head 结构（badge 上/名下）、meta `·` 分隔 + 脱敏 key 显示、
   chips border-top、grid 间距。

7. **ProviderEditor.tsx** —— 加 `.ml-scrim`（open 态）、抽屉 `.open` class + 关闭处理
   （取消按钮 / Esc / scrim 点击，Esc 挂全局 keydown 或 pane 内）；body 改 dgroup 分组；
   savebar 改 danger-text 删除 + 取消 + 保存。

8. **ModelRow.tsx** —— 折叠头重排（id 输入样式、推理 info 徽章、成本 `$i/$o`、test-pill）；
   展开体改 field-row 布局；fill 按钮 ghost。

9. **AgentTabs.tsx** —— agent-head 加名称、paradigm-strip、form-card 包裹、savebar 改圆角条。

10. **PresetList.tsx** —— agent-head + paradigm 条 + sec-head（标题+新增）；卡片 current
    描边、meta `provider → model`、动作 icon 化；warn-strip。

11. **UsageTab.tsx** —— toolbar 加标题 + window-switch 胶囊 + icon 刷新 + gen-at；图表两栏。

## 回归清单（每步后）

- [ ] `cd web && npx tsc --noEmit` 通过（或 `npm run build`）。
- [ ] 该组件在 dev 下的视觉/交互与设计稿对应区块一致；暗色模式无异常。
- [ ] 该组件现有功能行为无回归（保存/删除/测试/填充/同步/切换）。

## 风险点 / 回滚

- **抽屉定位**：`position:absolute` 依赖 `.pane-models` `position:relative`；若漏加会导致
  scrim/drawer 相对更上层容器定位。改后必测分屏/窄 pane。
- **`.ml-tabs` 圆角容器**：原为通栏条，改容器 padding 后若 body 顶部无 margin，视觉会贴顶；
  用 margin-bottom space-6 对齐设计稿。
- **test-pill 状态**：`runModelTest` 的异步态要改 className 路径（ok/fail/spin），
  注意按钮 disabled 期间不丢状态。
- **preset 动作改 icon**：删除按钮从「btn btn-danger btn-sm」变 icon 后，确认 confirm 弹窗
  仍触发；复制/编辑行为不变。
- 回滚：每完成一个提交节点即 commit，出问题 `git revert <commit>` 单点回滚。

## 完成判定

- prd.md A1–A10 全过；`npm run build` 绿；视觉 diff 对照设计稿验收。
- 完成后回 prd.md 勾选 AC；提交（Phase 3.4）。
