# implement.md — 父任务执行规划

> 本文件是**父任务**的执行规划，只编排子任务的顺序与集成验收。
> 具体实现步骤见各子任务的 implement.md。

## 任务地图

| 子任务 | 目录 | 交付物 | 状态 |
|---|---|---|---|
| 1 · Scenario 粒度重构 | `09-22-scenario-granularity-refactor/` | 新的 scenario 目录集 + 构建链路跑通 | 待办 |
| 2 · 运行时卷化 | `09-22-mise-runtime-volume-config/` | entrypoint symlink 种子 + 卷语义 | 待办（依赖 1） |

**依赖关系**：子任务 2 依赖子任务 1 —— R5 的 symlink 种子需要明确的目标路径
（`/opt/mise/installs/*`），该路径由子任务 1 的 L1 engine 拆分确定。
此依赖**不是树结构上的**，而是写在子任务 2 的 prd.md 中（Trellis 规范要求）。

---

## 阶段 0 · 准备（父任务直接负责）

- [ ] **0.1** 记录当前状态基线
  ```bash
  git rev-parse HEAD                    # 回滚锚点
  docker images --format '{{.Repository}}:{{.Tag}}\t{{.Size}}' | grep sandbox
  df -h /var/lib/docker                 # 确认 ≥25G 余量（见 memory: no-cache-rebuild-disk-full）
  cat .aio/enabled.toml                 # 记录当前选择，供迁移对照
  ```
- [ ] **0.2** 确认磁盘余量充足。若不足，按 memory `no-cache-rebuild-disk-full` 的
      「可回收性顺序」清理：`builder prune -af` → 孤儿 digest 镜像 → 无引用 `:latest`
      → 无挂载卷
- [ ] **0.3** **清理后立刻** `docker pull docker/dockerfile:1`（prune 会连带清掉
      BuildKit 前端镜像，不拉回则构建依赖外网，网络抖动即失败）

---

## 阶段 1 · Scenario 粒度重构（子任务 1）

**启动前**：确保子任务 1 的 prd.md / design.md / implement.md 齐备，且
`python3 ./.trellis/scripts/task.py start .trellis/tasks/09-22-scenario-granularity-refactor`

### 1.1 L1 mise engine 拆分（风险最高，先做）

先拆 engine，因为后续所有 mise fragment 都依赖它。

- [ ] 新建 `scenarios/mise/`（复用原目录名，语义变为 engine-only）
  - `scenario.toml`：`category = "os"`，`always_on = true`
  - `fragment.Dockerfile`：只保留 mise 二进制安装 + 四个 ENV + profile.d
    - **删除**原 fragment 中的 `[tools]` 段与 `mise install`
    - **保留** `ARG MISE_VERSION`
    - **保留** 双通道（ENV + profile.d）设计
- [ ] 验证：构建后 `mise --version` 可用，`/opt/mise/installs` 为空（对应 AC1）

### 1.2 L3 mise 派工具（rust / go / uv / ruff）

- [ ] 按 design.md §2.2 的模板 1 / 模板 2 建 4 个 scenario
- [ ] rust 用**模板 2**（table 形式 spec + 两个补偿）
- [ ] 验证 AC3：`rustc / cargo / clippy / rustfmt / rust-analyzer` 全可用

### 1.3 L2 shell 工具（10 个）

- [ ] 按模板 1 建 10 个 scenario：fzf / ripgrep / bat / fd / eza / zoxide /
      delta / starship / jq / yq
- [ ] **删除** `scenarios/shell-utils/`
- [ ] 验证 AC2：只选 fzf 时，其余 9 个不存在

### 1.4 L4 agent（3 个）

- [ ] 建 opencode / codex / claude-code 三个 scenario，`category = "app"`
- [ ] 验证 AC5：各自单独选中后命令可用
- [ ] 确认 `EnvPicker.tsx` 无需改动（不在 `SERVICE_SCENARIOS` 中，自动落 L4）

### 1.5 pi 改由 mise 装（保持服务区落位）

- [ ] 改 `scenarios/pi/fragment.Dockerfile`：CLI 本体由 `mise use -g pi@<ver>` 装
- [ ] **保留** `/opt/pi-extensions` 烘焙 + `aio-pi-extensions` 登记 + agent-browser
      wrapper/CLI 全部逻辑
- [ ] 验证 AC6

### 1.6 清理与迁移

- [ ] 更新仓库 `.aio/enabled.toml` 为新 id（D3）
- [ ] 全仓 grep 硬编码 `/usr/bin`、`/usr/local/bin` 路径（Q2）
- [ ] 验证 AC4：c23 与 mise 派同时选中互不干扰

### 1.7 集成验收

- [ ] `make config`（TUI）能正常展示新的分层与目录
- [ ] `cargo test`（config + mgr）全绿
- [ ] 完整构建：只选最小集 → 验证；全选 → 验证
- [ ] AC2 / AC3 / AC4 / AC5 / AC6 / AC8 / AC9

---

## 阶段 2 · 运行时卷化（子任务 2）

**启动前**：子任务 1 已合入，L1 engine 已独立。

- [ ] 见子任务 2 的 implement.md
- [ ] 关键验收 AC7（a/b/c 三项）

---

## 阶段 3 · 父任务集成收尾

- [ ] **3.1** 跨子任务验收对照：逐条核对父任务 prd.md 的 AC1–AC10
- [ ] **3.2** 端到端：新建一个 sandbox，从 TUI 选择 → 构建 → 创建 → 容器内验证
- [ ] **3.3** 离线分发验证 AC10：`make save` / `make load`
- [ ] **3.4** Spec 同步更新（Phase 3.3）
  - `.trellis/spec/backend/directory-structure.md`
  - `.trellis/spec/backend/api-contracts.md`
  - `.trellis/spec/backend/sandbox-mgr-ops.md`
  - `.trellis/spec/frontend/directory-structure.md`
  - `.trellis/spec/guides/cross-layer-thinking-guide.md`
- [ ] **3.5** README / README.zh-CN.md 的场景清单同步

---

## 风险文件与回滚点

| 文件 | 风险 | 回滚 |
|---|---|---|
| `scenarios/mise/fragment.Dockerfile` | 拆分后 engine 与工具混装 | 还原文件 + 重新 build |
| `scenarios/pi/fragment.Dockerfile` | mise 化后扩展机制断裂 | 还原 npm 装法 |
| `.aio/enabled.toml` | 新 id 写错导致 gen bail | 还原文件 |
| `app/entrypoint.sh`（子任务 2） | 卷语义错误导致工具不可用 | 还原 entrypoint + 重建容器 |

**整体回滚**：`git reset --hard <阶段 0.1 记录的 HEAD>` + 重新构建镜像。
注意 `mgr-data/state.db` 中的旧 sandbox 记录不受 git 管控，需手工处理。

---

## 启动前检查

- [ ] 父任务 prd.md / design.md / implement.md 齐备
- [ ] 两个子任务均已创建且建立了父子链接（`children` / `parent` 双向）
- [ ] 子任务 1 的 prd.md / design.md / implement.md 齐备
- [ ] `implement.jsonl` / `check.jsonl` 已填充真实条目（非 seed）
- [ ] 用户已 review 规划产物
