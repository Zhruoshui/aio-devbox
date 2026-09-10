# S1 创建向导重构：服务四开关 + 场景四层分组

父任务：09-10-mgr-web-ux-batch2（决策 D1/D2，全量背景与代码锚点见父 prd.md）。

## Goal

创建沙箱向导出现「服务」四开关（code-server/vnc/pi/pi-web，控制装与镜像
构建），场景选择按 L1-L4 四层分组展示并补 description。

## Requirements

- R1 create/edit API 扩展：请求体新增 services: {code_server, vnc, pi,
  pi_web}（默认全 true）；校验：pi_web=true 时强制 pi=true 且 vnc=true
- R2 envhash：服务开关纳入镜像组合哈希（不同开关组合不共享镜像）
- R3 jobs 构建条件化：code_server=false 不构建 sandbox-code-server-<hash>；
  vnc=false 不引用 sandbox-vnc；pi=false 时 pi 场景不进组合
  （pi_web=false 同理）；沙箱 compose 按开关省略对应服务段
- R4 EnvPicker 重构：分四节「系统 (L1)/Shell (L2)/语言 (L3)/应用 (L4)」
  双语标题；场景卡片显示 description；服务四项从场景区移除、聚合为独立
  服务开关区（含依赖提示）
- R5 沙箱列表/编辑页显示该沙箱服务组合；已建沙箱服务开关只读（不可改）

## Acceptance Criteria

- [ ] AC1 创建向导：服务开关区四项 + 场景四层分组 + 描述可见
- [ ] AC2 关闭全部服务创建的沙箱：compose 无对应服务段，镜像列表无
      code-server 镜像（该组合），面板按钮自动隐藏（manifest 探测）
- [ ] AC3 开 pi-web 不开 pi/vnc：前端禁用 + 后端 400
- [ ] AC4 旧沙箱（无 services 字段）列表/编辑/启动全流程不回归
- [ ] AC5 cargo test + tsc 全绿

## Notes

- design.md + implement.md 在本任务 task.py start 前补全
