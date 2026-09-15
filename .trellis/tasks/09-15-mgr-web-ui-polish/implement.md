# implement — mgr-web UI polish (issue #18)

前置:分支 `feat/mgr-web-ui-polish`,PR 目标 `main`。所有改动限于 `mgr-web/`。

## Step 1: 共享弹窗组件 (D1 基础)

- [ ] 新建 `mgr-web/src/components/Dialogs.tsx`:`ConfirmDialog` + `PromptDialog`,
      结构照 `SandboxListPage.tsx:253-302`(alertdialog)与 `RegisterDialog.tsx`(field/input、
      Esc/焦点管理)。样式只用 `components.css` 现有类,不写新 CSS。
- [ ] `PromptDialog` 回车提交、Esc 取消、打开聚焦并全选默认值、关闭恢复焦点。

验证:`npx tsc --noEmit` 通过。

## Step 2: 迁移 5 处原生弹窗 (D1 迁移)

- [ ] `ModelsPage.tsx:449` 导入确认 → ConfirmDialog(primary)
- [ ] `ModelsPage.tsx:474` 新建 profile → PromptDialog
- [ ] `ModelsPage.tsx:494` 重命名 → PromptDialog(defaultValue=current.name)
- [ ] `ModelsPage.tsx:516` 删除 profile → ConfirmDialog(danger)
- [ ] `PresetList.tsx:226` 删除预设 → ConfirmDialog(danger,组件内持有 state)
- [ ] `grep -rn "window.confirm\|window.prompt\|window.alert\|= confirm(\|= prompt(" mgr-web/src` 为 0

## Step 3: 删除页面级副标题 (D2)

- [ ] 删除以下 key 的双语值与所有渲染点(`<p className="sub">`):
      `listSub` `modelsSub` `usageSub` `edSub` `imgSub` `adSub` `wzSub` `wzNameSecSub` `mcProvidersSub`
- [ ] grep 确认无残留引用;`tsc --noEmit` 兜底(i18n key 删除后 `t()` 引用会编译失败)

## Step 4: 压缩其余冗长 hint (D2)

处置原则:纯机制解释 → 删;操作提示 → 压缩为 ≤1 短句;错误/状态/确认文案 → 保留。

| key | 处置 |
|---|---|
| wsEmpty | 保留(空状态指引) |
| dialogSub | 压缩 |
| fieldCmdHint / fieldPortHint | 保留核心格式提示,压缩 |
| probeDead / errPort / csStarting / csNotReady / jobFailedHint | 保留 |
| wzNameHint / wzScenariosHint / wzServicesHint / wzResHint | 压缩 |
| svcCsDesc / svcVncDesc / svcPiDesc / svcPiWebDesc | 保留(服务标识,非冗余) |
| adPathHint | 压缩 |
| sbExternalNote | 压缩 |
| mpUnassignedHint / mpAssignHint / mpAgentsHint / mpQuickAssignHint | 压缩或删 |
| maMgrNotice / maStripIncremental / maStripSwitcher | 压缩或删 |
| maSbxTblNote | 删或压缩 |
| mcKeyHint | 保留,压缩 |
| confirmDeleteSub / confirmUnadoptSub / sbVolLoss / muNoSandboxes | 保留(决策价值) |
| maParadigmIncremental / maParadigmSwitcher | 保留(术语标识) |

- [ ] 逐条按表处理,双语同步;实施中若发现表外同类长文案,按同一原则处理并记录

## Step 5: EN 溢出修复 (D3)

- [ ] `components.css:93` `.page-head .sub` — 若 Step 3 删完所有 `.sub` 后仍被其他页面用,加
      `min-width: 0; overflow-wrap: anywhere;`(防御)
- [ ] `components.css:226` `.dialog` — 加 `overflow-wrap: anywhere; max-height: 80vh; overflow: auto;`
- [ ] `.tree-hint` / `.dialog .desc` — `overflow-wrap: anywhere;`
- [ ] `.badge`(components.css:113)/ `.chip`(:176)— 加 `max-width` + ellipsis
- [ ] `.statusbar`(components.css:100)— 补 `text-overflow: ellipsis;`

验证:`npm run build` 通过;`npm run dev` 切 EN 检查各页无溢出。

## Step 6: 用量表对齐 (D4)

- [ ] `styles.css:1541-1555`:统一 th/td 垂直 padding
- [ ] `.ml-cell-clip` max-width 不对称问题:确认对应列 th 表现,必要时给该列 th 同策略
- [ ] 浏览器实测 sticky thead 滚动对齐;错位则 `.ml-usage-table` 改 `border-collapse: separate; border-spacing: 0`

## Step 7: 工作区按钮 (D5)

- [ ] 删除 `SandboxTree.tsx:170-179` "收起侧面板"按钮 + `panelOnToggle` prop + `WorkspacePage.tsx`
      传入点 + `hidePanel` i18n key(双语)
- [ ] `.ws-tree-foot`:gap → `--space-2`;去 `marginLeft: auto` 内联样式,`justify-content: space-between`
- [ ] EN 文案缩短:wsResetLayout → "Reset",manageSandboxes → "Manage"(zh 同步评估:
      "重置布局"→"重置"、"管理沙箱"→"管理",若 zh 不挤则保留原文)
- [ ] 浏览器实测 272px 面板内两种语言均单行横排

## Step 8: 全量验证

- [ ] `npm run build`(tsc --noEmit + vite build)通过
- [ ] 手工过查:① Models 页四种弹窗交互 ② 各页 EN/zh 无溢出 ③ 用量表对齐(滚动+不滚动)
      ④ 工作区按钮区 ⑤ Esc/scrim 关闭弹窗、焦点恢复
- [ ] dispatch trellis-check 做规格校验

## 提交与 PR

- [ ] Phase 3.3:trellis-update-spec(若有值得沉淀的契约,如"新弹窗必须用 Dialogs.tsx")
- [ ] commit(引用 issue #18),push,`gh pr create --fill`(body 带 `Closes #18`)
