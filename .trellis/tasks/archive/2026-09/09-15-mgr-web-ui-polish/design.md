# design — mgr-web UI polish (issue #18)

## 现状结论(来自代码摸底)

- 无共享弹窗组件;项目模式是内联 JSX + `components.css` 的 `.overlay`/`.dialog`/`.dialog-actions` 类。
  参考实现:`SandboxListPage.tsx:253-302`(删除沙箱,`role="alertdialog"`);`RegisterDialog.tsx`(表单弹窗,
  文件头注释记录了契约:条件渲染、Esc 关闭、焦点恢复)。
- 原生弹窗共 5 处:`ModelsPage.tsx:449/474/494/516`,`PresetList.tsx:226`。
- 冗长文案集中在 `i18n.ts`(zh 17-469 / en 471-891,同 key 双列),渲染容器多为 `.page-head .sub`、
  `.dialog .desc`、`.tree-hint`。完整清单见 implement.md 附表。
- EN 溢出根因:`.btn`/`.badge`/`.chip`/`.statusbar` 的 `white-space: nowrap` +
  272px 宽 `.ws-tree` 内 `.ws-tree-foot { flex-wrap: wrap }` 被长英文标签撑爆;
  `.page-head .sub` 在 flex 中被 `.page-actions { flex-shrink: 0 }` 压缩且无 `overflow-wrap`。
- 用量表对齐:单表(`UsagePage.tsx:391-457`),th 垂直 padding `--space-2` vs td `--space-1`;
  `ml-cell-clip` 的 max-width 只在 td;thead sticky + `border-collapse: collapse` 组合需浏览器实测。
- 工作区按钮:两个折叠按钮**功能不同**(见下);"重置布局/管理沙箱"横排问题是
  272px 面板内两个 nowrap 按钮 + 长英文标签导致 wrap 换行。

## D1 弹窗统一(R1)

新建共享组件 `src/components/Dialogs.tsx`(mgr-web 目前无 components 目录级共享弹窗,AgentAssignControl
是页面级组件;新建轻量组件符合 spec"copy from an existing one"的契约复用精神,避免 5 处各自内联):

- `<ConfirmDialog>`:props `{ open, title, desc?, danger?, confirmLabel?, cancelLabel?, onConfirm, onCancel }`。
  条件渲染 `.overlay.open` > `.dialog` > `.dialog-actions`(`btn-secondary` 取消 / `btn-danger` 或
  `btn-primary` 确认);破坏性确认 `role="alertdialog"`,普通确认 `role="dialog"`。
- `<PromptDialog>`:props 同上 + `{ defaultValue?, placeholder? }`,内含 `.field` + `.input`
  (样式参照 RegisterDialog);回车提交、Esc 取消;打开时聚焦输入框并选中默认值;关闭恢复焦点到触发元素
  (RegisterDialog 的 focus 管理模式,59-73 行)。
- 共同契约(照 spec component-guidelines):Esc 关闭、scrim 点击关闭、`e.stopPropagation()` 防穿透。
- 迁移点:
  - `ModelsPage` 导入确认 → ConfirmDialog(非危险,primary)
  - `ModelsPage` 新建 profile → PromptDialog;重命名 → PromptDialog(defaultValue=current.name)
  - `ModelsPage` 删除 profile → ConfirmDialog(danger,文案沿用 mpDeleteConfirm/mpDeleteConfirmAssigned)
  - `PresetList` 删除预设 → ConfirmDialog(danger)。注意该调用在组件内部,需在其宿主
    (ModelsPage)提升状态或在该组件内持有 dialog state——取组件内持有,最小改动。
- 完成后 `grep -rn "confirm(\|prompt(\|alert(" mgr-web/src` 仅允许出现自定义组件内部引用,原生调用为 0。

## D2 文案精简(R2)

原则(按用户反馈"纯机制解释无操作价值"):

- **删除**(用户已确认:页面级副标题全部删除):所有 `.page-head .sub` 纯描述文案——对应 UI 元素
  (`<p className="sub">`)与 i18n key 双语同删,`tsc` 门禁兜底死 key。清单:`listSub`、`modelsSub`、
  `usageSub`、`edSub`、`imgSub`、`adSub`、`wzSub`、`wzNameSecSub`、`mcProvidersSub`。
  附带效果:页头只剩标题与操作按钮,`.page-actions { flex-shrink: 0 }` 不再挤压标题区。
- **保留但压缩**:有操作指导价值的 hint(端口格式、命令示例、API key 注意事项等),压到 ≤1 短句。
  如 `fieldPortHint`、`fieldCmdHint`、`mcKeyHint`、`mpAssignHint`。
- **保留**:错误/状态提示(`errPort`、`probeDead`、`csNotReady`、`jobFailedHint`)与确认弹窗文案
  (`confirmDeleteSub` 等)——它们有明确的用户决策价值。
- 逐 key 处置表见 implement.md;实施时若发现清单外的同类长文案,按同一原则处理并在任务 journal 记录。

## D3 EN 溢出修复(R3)

目标:EN 模式下无文本撑破容器。定向修复(不加全局 reset,避免影响 golden-layout):

- `.page-head .sub`:加 `min-width: 0; overflow-wrap: anywhere;`(flex 压缩场景)。
- `.dialog`:加 `overflow-wrap: anywhere; max-height: 80vh; overflow: auto;`。
- `.tree-hint`、`.dialog .desc`:`overflow-wrap: anywhere;`。
- `.badge` / `.chip`:保留 nowrap 但加 `max-width` + `text-overflow: ellipsis`(徽章/芯片语义本就是单行截断,
  换行反而破坏视觉)。
- `.statusbar`:已有 overflow hidden,补 `text-overflow: ellipsis`。
- `.ml-table th` 的 nowrap 保留(配合 `.ml-table-scroll` 横向滚动是合理设计)。
- 工作区底部按钮溢出归入 D5。

## D4 用量表对齐(R4)

- 统一 th/td 垂直 padding(统一为 `--space-2` 或都为 `--space-1`,以视觉舒适为准)。
- `.ml-cell-clip` 的 `max-width` 移到通用 td 策略或同时作用于对应 th,消除不对称。
- 浏览器实测验证 sticky thead + border-collapse 组合;若滚动后错位,将 `.ml-usage-table` 改
  `border-collapse: separate; border-spacing: 0` 修复 sticky 边框/对齐问题。
- 验证方式:`npm run dev` + 浏览器(或构建后截图)确认中英两种语言下列对齐。

## D5 工作区按钮(R5)

- 用户已确认:**移除"收起侧面板"按钮**(SandboxTree.tsx:170-179),只保留"折叠沙箱树"。
  连带清理:`SandboxTree.tsx` 的 `panelOnToggle` prop 与 `WorkspacePage.tsx:666` 传入点、
  `hidePanel` i18n key(双语)。`App.tsx` 的 `mgr.panelHidden`/`togglePanel` 机制保留
  (导航栏点"工作区"仍有 goOrToggleWorkspace 行为,面板隐藏后仍有恢复通道,不动)。
- "重置布局/管理沙箱"横排:`.ws-tree-foot` 加大 gap 至 `--space-2`;缩短 EN 文案
  ("Reset layout"→"Reset","Manage sandboxes"→"Manage")并同步 zh;去掉 `marginLeft: auto` 内联样式,
  改 `justify-content: space-between`,保证 272px 面板内两种语言都单行放下。
  `tree-hint` 错误提示保持 `flex: 1 1 100%` 独占一行(仅出错时出现,不影响常规布局)。

## 兼容与回滚

- 纯前端 mgr-web 改动,无 API/数据契约变化;删除的 i18n key 无外部消费方(mgr-web 单一 SPA 消费)。
- `mgr.panelHidden`/`mgr.treeCollapsed` localStorage key 的去留取决于 D5 用户选择;若合并按钮,
  需迁移逻辑但不删 key(旧值自然失效即可)。
- 回滚 = revert 单个 PR;无数据迁移。
