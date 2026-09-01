# Tab 序号最小空闲号复用

## Goal

工作区 tab 的实例序号从「单调递增、只增不减」改为「关闭即回收，新实例取最小空闲号」。消除「关掉 (2) 再开同一个服务，新 tab 却叫 (3)」的反直觉现象。多实例能力（click again = another instance）保持不变。

## Background（已确认事实，来自代码勘察）

- tab 标题由 `web/src/App.tsx` 的 `launch()` 生成：每服务一个单调计数器 `seqRef`，`n===1` 显示裸 label，否则 `label (n)`（App.tsx:111-112, 240-247）。
- 关闭 tab 走 golden-layout `beforeComponentRelease` → React root unmount（App.tsx:278-284）；agent 类 pane 的 pty 会话被销毁（XtermPane.tsx:5-8 契约；后端 terminal.rs:185-193 kill+reap）。本任务不改销毁语义，只改编号。
- 刷新页面时 `resyncSeq()` 从持久化 layout 重建计数器，策略是取历史 max（App.tsx:557-568），导致序号跨会话膨胀。
- 冒烟测试 `web/smoke-test.cjs` 断言「点两次出现 Terminal (2)/(3)」与 reload 后标题集合一致（:144-151, :259-271）。

## Requirements

- R1 `launch` 时序号取该服务当前**未被在用**的最小正整数，而不是历史计数 +1。
- R2 组件释放（tab 关闭 / 拖出 popout / layout 销毁）时，该实例的序号归还空闲池。
- R3 页面刷新 / layout 恢复后，从恢复的 componentState 重建「在用序号集合」（替代现在的取 max 逻辑）。
- R4 已开着的 tab **不做活重命名**：只有新开实例复用被释放的号；正在显示的 tab 名字保持稳定。
- R5 `n===1` 显示裸 label、其余 `label (n)` 的惯例保持不变；同一服务的并发实例不重号（不产生同名 tab 歧义）。

## Acceptance Criteria

- [ ] AC1 打开服务两个实例（标 (2)、(3)），关闭 (2)，再点同服务按钮 → 新 tab 名为 `(2)`（而不是 (4)）。
- [ ] AC2 关闭全部实例后重开 → 新 tab 显示裸 label（复用 1 号）。
- [ ] AC3 开着 (1)(2) 时刷新页面 → 两 tab 标题不变；刷新后再点一次 → 新 tab 为 (3)。
- [ ] AC4 现有冒烟测试全绿（Terminal (2)/(3) 断言与 reload 断言不回归）；新增一条「关 (2) → 再开 → 仍为 (2)」的断言。
- [ ] AC5 `npm run typecheck` 通过。

## Out of Scope

- 不改「关闭 = 销毁会话」语义（pty / iframe 生命周期不动）。
- 不做「点击已开实例就聚焦」的 dedupe 行为。
- 不做活 tab 重命名 / 排序重编号。

## Open Questions

（无）
