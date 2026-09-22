# Scenario 粒度重构：L1 mise engine 必装 + L2/L3/L4 每工具可选

## Goal

把 scenario 从「整体开关」细化为「每工具一个 scenario」，让用户按需勾选每个工具而非
整套场景：

- **L1 os** — mise **engine** 升为 always_on（只给能力，不含任何工具，~30MB）
- **L2 shell** — shell 工具拆分到**每条**
- **L3 lang** — 分**两派**，每派有各自的配置方式（mise 管理 / 系统管理）
- **L4 app** — 新增 agent 层，收编 opencode / codex / claude-code

用户价值：不再为「想要 rust 就得吃下整套 1.9G mise」买单；镜像体积与所选工具精确对应。

## Background

### 现状（改动前）

`gen` 是纯字符串拼接：`Dockerfile.base.head + Σ(fragment, 按 layer 排序) + tail`。
scenario 是**全有全无**的整体开关，`scenario.toml` 只有 `id/name/description/category/
always_on/versions`。

L1/L2/L3/L4 的 category 已存在（`config/src/scenario.rs` 的 `CATEGORY_ORDER` +
`category_rank`），本次**不新增层级**，只是把各层内容做细。

### 关键实测结论（2026-09-22，本任务规划期）

以下均为在 `sandbox-base-c528612259c5` 容器内实跑所得，**非推测**：

| 验证项 | 结果 | 数据 |
|---|---|---|
| mise 能管 claude-code | ✅ | aqua 后端，2.1.278 |
| mise 能管 codex | ✅ | aqua 后端，0.155.1 |
| mise 能管 pi | ✅ | aqua 后端，0.86.1 / 109MB |
| mise 能管 L2 shell 工具 | ✅ | 10 个全成功，88 秒 / 60MB |
| **多 fragment 装 mise 工具不互相覆盖** | ✅ | **3 个独立 RUN 层 → config 完整合并** |
| mise 配置能分层（镜像级 + 用户级） | ✅ | `MISE_GLOBAL_CONFIG_FILE` 指向卷后两者同时生效 |
| 烘焙工具 + 用户自装可共存于卷 | ✅ | **卷只占 3.7M**（symlink 引用镜像层） |

**「多 fragment 不覆盖」是本设计的关键依据**：三个独立 RUN 层各跑 `mise use -g <tool>`
后，`/opt/mise/config.toml` 的 `[tools]` 段是三者并集：

```toml
[tools]
fd = "10.5.0"
jq = "1.8.2"
starship = "1.26.0"
```

因此**不需要发明新的 gen 聚合机制**——每个工具独立成一个 scenario、各自
`mise use -g`，天然幂等可组合。这否决了规划早期设想的「gen 横向聚合 + 占位符注入」方案。

### 技术约束

- **L3 的两派是并存的两个 scenario**，不是二选一：
  - 派 A（mise 管理）：rust / go / uv / ruff 各自独立
  - 派 B（系统管理）：c23，apt 装系统路径
  - 两者平级，用户可以都要（rust 写库 + clang 写 C）——这正是「两派」的含义。
- **mise 的 clang 不可用**：实测走 `conda:clang` 后端，target 为
  `x86_64-conda-linux-gnu`（非 `pc-linux-gnu`），自带一整套 sysroot。且 c23 需要的
  `clang-tidy / clangd / lld / gdb / valgrind / cppcheck / strace` 在 mise registry
  中**完全不存在**。故 C 链路保持 apt.llvm.org 不动。

## 设计决策

### D1 — 用「每工具一个 scenario」表达条目级选择

**决策**：不为 scenario 新增 `[[items]]` 子条目机制。每个工具就是一个独立的
`scenarios/<tool>/`（含 `scenario.toml` + `fragment.Dockerfile`）。

**理由**：
1. 现有 `category` 分层（os/shell/lang/app）已提供分组能力，UI 天然按 layer 渲染；
2. `gen` / `tui` / `manifest` / `EnvPicker` / `envhash` **全部零改动**——
   scenario 本就是最小可选单元，细化粒度只是增加目录数量；
3. 上面「多 fragment 不互相覆盖」的实测结果，正是让这条路走得通的前提；
4. 替代方案（`[[items]]` 子条目）需要改 6 个模块，而收益仅是 UI 上多一层折叠，
   在只有十几个工具的规模下不划算。

**代价**：`scenarios/` 目录数量从 8 个增至约 24 个。可接受——目录即清单，
比嵌套结构更易读易 diff。

### D2 — agent 的 UI 落位：L4 场景区（pi 除外）

**决策**：opencode / codex / claude-code 进入 L4 场景区（`EnvPicker`）；
**pi 保持在服务区（`ServicesPicker`）不动**。

**理由**：`EnvPicker.tsx` 的 `SERVICE_SCENARIOS = ["pi","pi-web"]` 是 S1 已确立的决策
——「一个开关只出现在一个地方」。pi 必须与服务区的 pi-web 同处一地，因为二者有级联
依赖（开 pi-web 自动开 pi，关 pi 级联关 pi-web，`routes.rs normalize_services`）。
其余 agent 没有这类耦合，归属 L4 场景区更自然。

**影响**：L4 场景区含 3 项；服务区保持 code-server / vnc / pi / pi-web 四开关不变。

### D3 — 旧配置直接删，不保留兼容壳

**决策**：`scenarios/shell-utils` 直接删除（`scenarios/mise` 复用为 engine），
**不保留空壳兼容**。仓库 `.aio/enabled.toml` 同步改写为新 id；现有 sandbox `feiver`
的旧 env 不迁移（场景重构后镜像 hash 必然变化，重建时本就要换新环境）。

**理由**：保留空壳会在 catalog 里留下无意义条目，且掩盖「旧配置已失效」的事实。
只有 1 个 sandbox 且必然需要重建，迁移成本为零。

## Requirements

### R1 — L1 mise engine（给能力，不给工具）

- R1.1 mise **engine**（二进制 + shims + 四个 ENV 重定向 + profile.d activate）成为
  always_on，**不包含任何工具**。
- R1.2 拆出现有 `scenarios/mise`：复用其目录与 id，但语义收窄为 engine-only；
  工具部分移 L3 派 A。
- R1.3 L1 必装带来的镜像增量应约为 engine 二进制量级（~30MB），不得因必装而暴涨。

### R2 — L2 shell 工具，每工具一个 scenario

- R2.1 每个 shell 工具一个独立 scenario（`category = "shell"`）：fzf / ripgrep / bat /
  fd / eza / zoxide / delta / starship / jq / yq。
- R2.2 统一走 mise（实测 10 个全有 aqua 后端），fragment 内各自
  `mise use -g <tool>@<ver>`。
- R2.3 删除现有 `scenarios/shell-utils`（整包拆分后不再需要）。
- R2.4 移除原 shell-utils 中为 Debian 改名所做的 `fdfind`/`batcat` 软链逻辑
  （mise 安装的工具名即规范名，无需软链）。

### R3 — L3 分两派

- R3.1 **派 A（mise 管理，`category = "lang"`）**：rust / go / uv / ruff 各为一个独立
  scenario，由 mise 装到 `/opt/mise`。
- R3.2 **派 B（系统管理，`category = "lang"`）**：c23 保持为一个 scenario，apt 装
  系统路径（clang-22 + lld + compiler-rt + 配套工具）。
- R3.3 两派互不替代、互不冲突；用户可同时选中。
- R3.4 rust 的 fragment 必须保留 `profile = "default"` 与
  `mise exec -- rustup component add rust-analyzer` 两步补偿
  （否则丢 clippy/rustfmt；缺 rust-analyzer 时 rustup 代理与 shim 会形成无限递归）。
- R3.5 因 `mise use -g` 只能写简单形式 `tool = "ver"`，rust 需 table 形式
  `{ version = "...", profile = "default" }`，其 fragment 须**直接写 config** 而非
  用 `mise use -g`。

### R4 — 新增 L4 agent 层

- R4.1 新建 `category = "app"` 的 agent scenarios：**opencode / codex / claude-code**
  各一个，由 mise 安装。
- R4.2 每个 agent 独立勾选。
- R4.3 **pi 的落位不变**：继续作为 scenario（`id = "pi"`），继续由服务区承载开关，
  不移入 L4 场景区（理由见 D2）。
- R4.4 pi 由 npm 全局安装改为 mise 管理（CLI 本体）。
- R4.5 pi 的扩展烘焙机制（`/opt/pi-extensions` + `aio-pi-extensions` 零网络登记
  + agent-browser wrapper/CLI）**必须原样保留**，mise 只接管 CLI 本体。

### R5 — 运行时自由配置

- R5.1 用户可在容器内 `mise use -g <tool>` 自装工具，且**跨 recreate 存活**。
- R5.2 自装不得复制镜像烘焙的 1.9G 内容进卷。

> R5 由子任务 `09-22-mise-runtime-volume-config` 实现，本父任务只定义需求与验收。

## Acceptance Criteria

- [ ] AC1 — L1 构建后 `mise --version` 可用，且 `/opt/mise/installs` 中**无任何工具**
- [ ] AC2 — 只勾选 fzf 构建后，fzf 可用、其余 9 个 shell 工具**不存在**
- [ ] AC3 — 勾选 rust 后 `rustc/cargo/clippy/rustfmt/rust-analyzer` 全部可用
- [ ] AC4 — c23 的 `clang -std=c23` 冒烟编译运行通过，且与 mise 派同时选中时互不干扰
- [ ] AC5 — 每个 agent 单独勾选后对应命令可用（`opencode --version` /
      `claude --version` / `codex --version`）
- [ ] AC6 — pi 由 mise 管理后，`aio-pi-extensions` 登记流程仍可用
- [ ] AC7 — 容器内 `mise use -g <新工具>` 后（a）立即可用；（b）`docker restart`
      后仍可用；（c）卷体积增量仅为该工具本身，不含烘焙内容副本
- [ ] AC8 — 不同 scenario 选择产生不同 `env_hash`（自动满足，需验证）
- [ ] AC9 — TUI 与 mgr-web 均能展示并操作新的分层场景列表
- [ ] AC10 — 离线分发（`make save/load`）路径不被破坏

## Out of Scope

- **C 链路迁 mise**（实测不可行，见技术约束）
- **node / python 迁 mise**（保持现有 tarball 安装，不碰已稳定链路）
- agent-browser 插件与 pi CLI 的版本耦合问题（用户明确「先不管」）
- L5 service 层

## Open Questions

- Q1 — mise 工具落在 `/opt/mise/shims` 而非 `/usr/bin`。shims 已在 PATH，交互 shell
  无影响；需确认无脚本硬编码系统路径（现有 shell-utils 的 `ln -sf ... /usr/local/bin/fd`
  就是这类处理，拆分后删除）。
- Q2 — agent 二进制（claude-code / codex）为闭源商业工具，烘进镜像后随 `make save`
  分发的合规性需用户确认（**不阻塞实现，仅提示**）。

## Notes

- 关键文件：`config/src/scenario.rs`、`config/src/gen.rs`、`config/src/tui.rs`、
  `config/src/manifest.rs`、`mgr/src/envhash.rs`、`mgr-web/src/pages/EnvPicker.tsx`
- 子任务：
  - `09-22-scenario-granularity-refactor`（构建链路重构）
  - `09-22-mise-runtime-volume-config`（R5 运行时卷化，依赖前者）
