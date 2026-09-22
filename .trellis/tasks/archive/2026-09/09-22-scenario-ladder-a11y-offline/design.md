# 设计：L3 阶梯分组 + 可见性修复 + 离线验证

## D1 — 用结构化 `installer` 字段表达「安装方式」

**决策**：在 `ScenarioMeta`（`config/src/scenario.rs`）新增

```rust
#[serde(default = "default_installer")]
pub installer: String,
```

取值 `"mise"` / `"apt"` / `"npm"` / `"tarball"`。`default_installer()` 返回 `"mise"`
（粒度重构后绝大多数工具走 mise；与既有 `default_category` 的 back-compat 思路一致）。

**理由**：
1. 安装方式目前只活在 `description` 的自然语言里。要从描述解析出分组是脆的
   （改文案就崩），必须在数据模型里结构化。
2. 阶梯的语义正是父任务 PRD R3 的「两派」：派 A mise 管理 / 派 B 系统管理。
   把这条早已存在的设计意图落成字段，是补上欠账而非新增概念。
3. `#[serde(default)]` 保证所有现存 `scenario.toml` 不改也能解析 —— 但本次仍
   显式补齐全部 23 个文件，让清单自解释（目录即清单，父任务 D1 的既定风格）。

**代价**：`gen` / `manifest` / `envhash` 零改动（不读该字段）；只需 tui.rs 与
EnvPicker.tsx 各加一层分组渲染。

**替代方案（否决）**：前端按 `category == "lang" && id == "c23"` 硬编码。
否决理由：c23 不是唯一特性，fonts/node/python/pi 各有安装方式，硬编码会随
场景增加而腐烂，且 TUI 无法复用。

## D2 — 阶梯只在「同层多方式」时渲染

**决策**：分组键为 `(category, installer)`；若某 category 下的 installer 去重后
**只有一种**，则该层不渲染子标题（保持现状）。

**理由**：L2 shell 全 10 个走 mise、L4 app 走 mise/npm —— 给它们加子标题是纯噪声。
只有 L3（mise ×4 + apt ×1）与 L1（tarball ×2 + apt ×1 + mise ×1）真正需要分阶。

## D3 — 可见性修复走「全量悬空引用审计」

**决策**：不只补 `--elev-ring`，而是写一个审计脚本比对
`var(--X)` 引用集合 与 `--X:` 定义集合，差集必须为空。

**理由**：`--elev-ring` 被删说明主题重构时有系统性疏漏。只修症状（.check）
不修根因（悬空引用无人检测），下次重构还会复发。审计脚本同时作为回归闸门。

**修复位置**：`--elev-ring` 定义放 `:root`（模式无关原语区），值取
`0 0 0 1px var(--border)` —— `--border` 是每主题 token，故 8 套主题自动生效。

## D4 — 离线验证分三层，逐层给结论

| 层 | 手段 | 回答的问题 |
|---|---|---|
| L-a 机制 | `make save` → `make load` 端到端 | 分发链路是否被粒度重构破坏 |
| L-b 功能 | `docker run --network none` 跑 enabled.toml 全场景探针 | 烘焙的环境配置离线是否可用 |
| L-c 边界 | 离线容器内 `mise use -g <新工具>` | 运行期自装能否离线成立 |

**磁盘约束（关键）**：`/var/lib/docker` 仅剩 11G，而默认
`SAVE_IMAGES = sandbox-base sandbox-app sandbox-code-server sandbox-vnc caddy:2`
加起来 >25GB（增量），直接 `make save` 会写满磁盘。

**处置**：先清理两组**已被取代的旧 envhash 变体镜像**
（`sandbox-*-c528612259c5` / `sandbox-*-df882efe5ef5`），它们是粒度重构前的
历史变体、compose 不再引用；确认无容器占用后删除以腾出空间。

**no-rollback 风险点**：删除镜像是不可逆操作。缓解：删除前逐条核对
`docker ps -a` 无引用、`docker compose config` 不引用；且这些变体可由
`make gen + make build-base` 重建。

## D5 — 文档落点

离线边界结论写入 `docs/offline-install-guide.md`（已有 §0–§4 结构，补一节
「mise 卷化后的离线语义」），不新建文档 —— 用户查阅入口保持单一。
