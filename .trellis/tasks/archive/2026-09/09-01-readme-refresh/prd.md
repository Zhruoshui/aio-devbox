# README 重构（en + zh 同步更新）

## Goal

README.md / README.zh-CN.md 与仓库现状失同步；重构两版使其准确、精炼。

## Requirements

### 需要同步的事实差异（核对自仓库现状）

1. **场景表缺 4 个场景**：`c23`（clang-22 C23 工具链，L3 lang）、`fonts`
   （Maple Mono NF CN，L1 os）、`pi`（AI agent，L4 app）、`pi-web`（pi 浏览器
   UI，L4 app）。
2. **Node 版本列表过期**：实际为 20.18.0 / 22.23.2 / 22.11.0 / 18.20.4，
   默认 22.23.2。
3. **按钮体系**：services.toml 新增 `type = "page"`（原生 React 面板，
   "模型配置" 按钮）；`piWeb` 是 web 按钮但走 app 直发端口 30141
   （`{host}` 占位符 + entrypoint 自启），不走网关子路径。
4. **Makefile 目标表缺 `make save` / `make load`**（离线 bundle：镜像 +
   .env + hash + enabled.toml）。
5. **`.aio/presets/` 双预设文件**（minimal.toml / full.toml）+ 选区通配符
   `scenarios = ["*"]` 机制（gen 展开为所有非 always_on 场景）未提及。
6. **`Dockerfile.base` 已移出版本控制**（991849a，生成物），Project layout
   描述需更新。
7. **pi 场景离线分发**：`aio-pi-extensions` 需在终端手动跑一次（make load
   提示语里有），pi 扩展烘 /opt + 零网络登记。

### 写作要求

- 避免长篇大论：README 保持入口文档定位，细节留给代码注释与 docs/。
- 关键处（场景表、按钮自动探测规则、netns 共享端口约束、make 工作流、
  离线路径）认真、准确编写。
- en 与 zh 两版内容结构一致、事实一致。

## Acceptance Criteria

- [x] 场景表与 `scenarios/*/scenario.toml` 一一对应（13 个场景全部列出）。
- [x] Node/Python 版本与 scenario.toml 一致。
- [x] 按钮三类型（web/agent/page）均有描述；piWeb 直发端口 30141 有说明。
- [x] Makefile 目标表覆盖全部 .PHONY 目标（含 save/load/pull）。
- [x] `.aio/presets/` + 通配符机制有说明；Dockerfile.base 生成物状态注明。
- [x] en/zh 两版同步修改，无一方落后。
- [x] 篇幅不显著膨胀（README 主体行数与现在相当或更少）。
