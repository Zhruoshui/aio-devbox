# Scenario 粒度重构：L1 mise engine 拆出 + L2/L3/L4 每工具一个 scenario

> 父任务：`.trellis/tasks/09-22-scenario-items-layered-mise/`（需求与架构见其 prd.md / design.md）

## Goal

把 scenario 从「整体开关」细化为「每工具一个 scenario」，具体交付：

1. **L1** — mise **engine** 独立成 always_on scenario，**不含任何工具**
2. **L2** — 10 个 shell 工具各自一个 scenario
3. **L3** — mise 派 4 个工具（rust/go/uv/ruff）+ 系统派 c23 保持
4. **L4** — 新增 opencode / codex / claude-code 三个 agent scenario
5. **pi** — CLI 本体改由 mise 装，**服务区落位不变**
6. **清理** — 删除 `scenarios/shell-utils`，改写仓库 `.aio/enabled.toml`

## Background

### 为什么可行（关键实测，2026-09-22）

**三个独立 RUN 层各跑 `mise use -g <tool>`，`/opt/mise/config.toml` 的 `[tools]` 段
是并集，无覆盖**：

```toml
[tools]
fd = "10.5.0"
jq = "1.8.2"
starship = "1.26.0"
```

`mise use -g` 是读改写语义（读现有 config → 合并 → 写回），对 Docker 顺序层幂等可组合。
**因此本任务不需要改动 `gen`**——纯字符串拼接模型完全适用。

### 实测数据

| 项 | 结果 |
|---|---|
| L2 工具 10 个（fzf/ripgrep/bat/fd/eza/zoxide/delta/starship/jq/yq） | 全部装成功，88 秒 / 60MB |
| claude-code | aqua 后端，2.1.278 |
| codex | aqua 后端，0.155.1 |
| pi | aqua 后端，0.86.1 / 109MB |
| L4 全套体积 | 862MB（opencode 177 + claude-code 224 + codex 354 + pi 109） |

### 现有 fragment 参考

- `scenarios/mise/fragment.Dockerfile` — engine 部分照搬，`[tools]` 段删除
- `scenarios/shell-utils/fragment.Dockerfile` — apt 装法，改为 mise 后删除软链逻辑
- `scenarios/pi/fragment.Dockerfile` — 扩展机制需完整保留
- `scenarios/c23/fragment.Dockerfile` — 不动

## Requirements

### 1. L1 mise engine（`scenarios/mise/`）

- 1.1 `category = "os"`，`always_on = true`
- 1.2 fragment 只含：mise 二进制 + 四个 ENV 重定向 + profile.d activate
- 1.3 **不得**包含 `[tools]` 段或 `mise install`
- 1.4 保留双通道设计（ENV 覆盖非 login shell / profile.d 补偿 login shell）
- 1.5 保留 `ARG MISE_VERSION` 以固定版本
- 1.6 保留双通道自检（login + non-login 各验一次）

### 2. L3 mise 派（4 个 scenario）

- 2.1 rust / go / uv / ruff 各一个目录，`category = "lang"`
- 2.2 每个带 `[[versions]]`（沿用原 mise 场景的版本号）
- 2.3 **rust 特殊**：`mise use -g` 只能写 `tool = "ver"` 简单形式，rust 需要
      `{ version = "...", profile = "default" }` table 形式 → **直接写 config**：
      ```dockerfile
      RUN printf 'rust = { version = "%s", profile = "default" }\n' "$RUST_VERSION" \
            >> /opt/mise/config.toml \
       && mise install rust \
       && mise exec -- rustup component add rust-analyzer
      ```
      两个补偿缺一不可（缺 `profile` 丢 clippy/rustfmt；缺 rust-analyzer 会因
      rustup 代理与 shim 互相回调**无限递归**，PoC 实测）

### 3. L2 shell（10 个 scenario）

- 3.1 fzf / ripgrep / bat / fd / eza / zoxide / delta / starship / jq / yq，
      `category = "shell"`
- 3.2 统一模板：`ARG <TOOL>_VERSION=x.y.z` + `mise use -g "<tool>@${VERSION}"` +
      `command -v` 自检 + `--version` 冒烟
- 3.3 **删除** `scenarios/shell-utils/`
- 3.4 不保留 `fdfind`/`batcat` 软链（mise 装的即规范名）

### 4. L4 agent（3 个 scenario）

- 4.1 opencode / codex / claude-code，`category = "app"`
- 4.2 每个一个独立 scenario，mise 安装
- 4.3 验证 `EnvPicker.tsx` 的 `SERVICE_SCENARIOS = ["pi","pi-web"]` **无需修改**，
      新 agent 自动落入 L4 场景区

### 5. pi 改 mise 装（落位不变）

- 5.1 `scenarios/pi/scenario.toml` 的 `category = "app"` **不变**
- 5.2 fragment 中 CLI 本体改为 `mise use -g pi@<ver>`
- 5.3 **完整保留**：`/opt/pi-extensions` 烘焙、`aio-pi-extensions` 登记脚本、
      agent-browser CLI 烘焙 + wrapper、`pi-agent-browser-doctor/config` 软链
- 5.4 `pi-web` scenario 不动

### 6. 迁移

- 6.1 更新仓库 `.aio/enabled.toml` 为新 id（`mise` 保留但语义变 engine；
      `shell-utils` 替换为 10 个新 id）
- 6.2 全仓 grep 硬编码系统路径（父任务 Q2）

## Acceptance Criteria

- [ ] **AC1** — 构建后 `mise --version` 可用，且 `/opt/mise/installs` **为空**
- [ ] **AC2** — 只选 `fzf` 构建后，`fzf` 可用、其余 9 个 shell 工具**不存在**
- [ ] **AC3** — 选 `rust` 后 `rustc / cargo / clippy / rustfmt / rust-analyzer` 全可用
- [ ] **AC4** — c23 的 `clang -std=c23` 冒烟编译运行通过；与 mise 派同时选中互不干扰
- [ ] **AC5** — 每个 agent 单独选中后命令可用（`opencode --version` /
      `claude --version` / `codex --version`）
- [ ] **AC6** — pi 由 mise 装后 `aio-pi-extensions` 登记流程仍可用
- [ ] **AC8** — 同一 scenario 选不同工具产生不同 `env_hash`
- [ ] **AC9** — TUI 能正常展示新的四层分组与全部目录
- [ ] **AC11** — `cargo test`（config + mgr）全绿
- [ ] **AC12** — 仓库 `.aio/enabled.toml` 已更新，`make config` 后 `gen` 不 bail

## Out of Scope

- **R5 运行时卷化**（子任务 2）
- C 链路迁 mise（实测不可行）
- node / python 迁 mise
- pi 与 agent-browser 的版本耦合（用户明确「先不管」）
- 离线分发（AC10）由父任务阶段 3 统一验证

## Notes

- **不改动的文件**（已在父任务 design.md §3.1 逐一确认）：`config/src/*.rs` 全部、
  `mgr/src/envhash.rs`、`mgr-web/src/types.ts`
- 风险最高的是 **1.1（L1 engine 拆分）**，建议先做并单独验证，再展开其余
- 磁盘：完整构建前确认 `/var/lib/docker` ≥25G 余量
