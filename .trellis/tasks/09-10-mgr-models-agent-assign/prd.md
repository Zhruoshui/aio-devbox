# S2 模型配置重构：agent 指派三层结构

父任务：09-10-mgr-web-ux-batch2（决策 D4，全量背景与代码锚点见父 prd.md）。

## Goal

模型配置按 cc-switch 心智重构：全局供应商库（保持）→ profile × agent
卡片式指派 → 沙箱指派 profile + agent 子集（只渲染勾选的 agent）。

## Requirements

- R1 Models 页 agent 区块强化：四 agent（pi/claude/codex/opencode）各自
  卡片式一键切换指向供应商库项（点卡片=激活）；复用 aio-models
  AgentsConfig（本就 per-agent）
- R2 沙箱指派扩展：PUT /api/sandboxes/:name/model_profile 增 agents 子集
  参数（默认全部四 agent）；EditPage/SandboxListPage 指派 UI 含 agent 勾选
- R3 mgr_sync 携带 agent 集合：沙箱拉取后只渲染被指派 agent 的配置文件
  （未指派 agent 的本地配置不动）
- R4 渲染器复用 app/src/routes/models/render/ 四件套，零格式适配；
  各 agent 配置文件规范以现有渲染器为唯一事实源

## Acceptance Criteria

- [ ] AC1 Models 页四 agent 卡片切换可用，激活态清晰
- [ ] AC2 沙箱 A 指派 profile P + 仅 pi/opencode：60s 内 ~/.pi 配置更新、
      claude/codex 配置文件不变
- [ ] AC3 未指派任何 agent 的沙箱：拉取后本地模型配置完全不动
- [ ] AC4 旧指派数据（无 agents 字段=全指派）不回归
- [ ] AC5 cargo test + tsc 全绿

## Notes

- design.md + implement.md 在本任务 task.py start 前补全
