# S5 侧栏折叠：图标栏 + flyout

父任务：09-10-mgr-web-ux-batch2（决策 D6，全量背景见父 prd.md）。

## Goal

管理器左侧导航可折叠为图标栏；工作区 SandboxTree 可折成竖向图标条 +
hover flyout，最大化工作区。

## Requirements

- R1 App.tsx sidebar 折叠态：只露图标（~48px），展开/收起按钮常驻，
  状态存 localStorage（键如 aio.mgr.sidebarCollapsed）
- R2 SandboxTree 折叠态：竖向细条，每沙箱一个首字母圆形图标；hover 弹
  flyout 浮层显示该沙箱服务按钮组（终端/agent/code-server/vnc/pi-web，
  按钮可见性与展开态一致——manifest 探测）；点击按钮即开 pane 并不关闭
  flyout（连续开多个）
- R3 折叠状态下 stopped 沙箱的置灰/启动入口语义保持（flyout 内同样置灰）
- R4 golden-layout tab 条不动；两个折叠互不影响、各自记忆
- R5 响应式：flyout 超出视口时自动翻转方向

## Acceptance Criteria

- [ ] AC1 管理器侧栏折叠/展开流畅，刷新后状态保持
- [ ] AC2 SandboxTree 折叠后 hover 任意沙箱图标弹 flyout，点击按钮开 pane
- [ ] AC3 折叠态 stopped 沙箱按钮置灰且提供启动
- [ ] AC4 展开态行为与现状完全一致（回归）
- [ ] AC5 tsc + build 全绿（纯前端，无后端改动）

## Notes

- 轻量任务：PRD-only 即可启动，不强制 design.md
