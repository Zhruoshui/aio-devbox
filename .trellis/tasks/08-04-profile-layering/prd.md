# Profile 四层分层模型重构

## Goal

把当前**扁平、单一机制**的场景预置系统，重构为**四层分层模型**，让不同性质的定制（系统包 / shell 命令 / 语言开发链路 / 应用与 AI agent）各归其位，TUI 与装配器按层组织与表达。

## 用户提出的四层（语义待确认）

- **L1 操作系统软件包层**：预装应用、基础环境（apt 包、node、opencode 等）。
- **L2 好用的 shell 命令层**：别名/函数/completion/常用脚本。
- **L3 语言最佳实践开发配置层**：语言工具链 + LSP + formatter + linter（即现有 rust/python-dev/go 场景）。
- **L4 应用层**：各种应用，包括 AI agent 等。

## Background（代码已确认的事实）

- 现状为**单一机制、扁平结构**：`scenarios/<id>/` 每个含 `fragment.Dockerfile`（裸 Dockerfile 片段）+ `scenario.toml`（id/name/description）。现有三个场景 rust/python-dev/go **全部属于 L3**。
- **L1 目前硬编码在 `Dockerfile.base.head`**：HTTPS apt 源 sed、ca-certificates 自举、apt 装 curl/git/gnupg2/xz/python3/pip/venv/build-essential/pkg-config/libssl-dev/locales/tzdata/sudo、node 20（nodejs.org tarball）、opencode（GitHub release）、建用户 `gem`(uid 1000)。TUI 不碰，不可勾选。
- **L4 运行时服务已存在于 compose**：code-server、vnc、app(axum)、gateway(caddy) 是独立容器，`FROM sandbox-base`。AI agent CLI 目前仅 opencode 烘在 head。
- **L2 完全不存在**：无 dotfiles / aliases / `/etc/profile.d` 机制。
- **机制边界**：所有场景片段均为 **build-time、以 root 执行、落系统路径**（`/opt`、`/usr/local`，必要时 `chown gem`），避开共享卷 `aio_workspace` 对 `/home/gem` 的卷遮盖；运行时补装仍走 `~/.local/bin`（`docs/offline-install-guide.md` 既有分工）。
- **装配器** `config/src/gen.rs`：纯拼接 `head` + Σ(按 id 字母序)`fragment.Dockerfile` + `tail`，幂等。
- **manifest** `.aio/enabled.toml`：`Enabled{ scenarios: Vec<String> }`，TUI 写、gen 读。
- **scenario 发现** `config/src/scenario.rs`：扫 `scenarios/*/scenario.toml`，校验 id==目录名、fragment 存在。
- **离线哲学**：base 在联网机 gen+build → `docker save` → 离线机 `load` → `make up NOBUILD=1`。改选 = 回联网机重建。
- **TUI 现状**（`config/src/tui.rs`）：扁平 checkbox 列表，`scenarios/*/scenario.toml` 全平铺，`[x] name  description` 一行一个，Space 切换 / `s` 存 / `q` 弃 / ↑↓ 移。**无 category/分组概念**，`scenario.toml` 也无 `category` 字段，标题硬编码。TUI 只扫 `scenarios/`，对 `Dockerfile.base.head`（L1）一无所知——L1 当前不进 TUI。

## Resolved

- **Q1 = 统一机制 + `category` 标签**：L1–L4 全部是 build-time Dockerfile 片段（烘进 `Dockerfile.base` 系统路径），机制不变；`scenario.toml` 增 `category` 字段，TUI 按层分组。L2 的 shell 命令落 `/etc/profile.d/*.sh`（login shell 全局 source，不被共享卷遮盖）与 `/usr/local/bin`。L4 的"应用"在此层只指 CLI 应用（含 AI agent CLI）；**长驻服务（code-server/vnc/app/gateway）已是 compose 服务，不进 scenario 系统**。**L5（compose/docker 外部服务接入）为未来扩展，本任务不做**，但设计要留扩展位（category 枚举可后加、装配器不锁死四层）。
- **Q2 = (A) L1 不进 TUI**：L1 保持 `Dockerfile.base.head` 常驻、不可勾选；TUI 勾选范围仅 L2/L3/L4。理由：`sandbox-base` 被所有 compose 服务 `FROM` 继承，L1 包是依赖项（curl 给 L3、node 给 code-server、build-essential 给编译），暴露成可勾选会跨容器坏（取消 node -> code-server 死）；勾选语义是"个人开发偏好"，L1 是"地基依赖"而非偏好。**已实现为可行性 spike（见下）**。

## Implemented（可行性 spike，已落地并验证）

实现 Q1+Q2(A) 的最小机制，证明四层用"统一 build-time 片段 + category 标签"可行：

- `config/src/scenario.rs`：`ScenarioMeta` 增 `category: String`（`#[serde(default = "default_category")]`，默认 `"lang"`，向后兼容旧 toml）；新增 `category_rank()`（`os<shell<lang<app<service`，未知层排末尾）与 `category_title()`（TUI 分组标题）。附 3 个单元测试（默认值/字段/排序）。
- `config/src/tui.rs`：扁平列表改为**按 category 分组**--`Row::{Header, Item}` 交错，Header 不可勾选、Up/Down 跨行、Space 仅对 Item 生效；`checked` 仍按 scenario 索引，插 Header 不影响勾选状态。标题改 "AIO 开发场景 · 分层"。
- 三个现有场景 toml 加 `category = "lang"`。
- 新增 **L2 示例场景** `scenarios/shell-utils/`：apt 装 fzf/ripgrep/bat/fd-find（+ Debian 改名软链 fd->fdfind、bat->batcat）+ `RUN cat > /etc/profile.d/aio-shell.sh <<'EOF'` 落全局别名（ll/la/..//g/d/dc/grep）。

**验证结果**：`make build-config` 通过；`cargo test` 6/6 过；`make gen` 正确拼装含 shell-utils 的 `Dockerfile.base`（heredoc 完整保留）；只启用 shell-utils 重建 `sandbox-base` 成功；进容器验证 fzf/rg/bat/fd 全可用、`/etc/profile.d/aio-shell.sh` 落地、三类 shell 形态别名均定义且实跑成功（见下"终端覆盖"）。TUI 在无 tty 环境 smoke 到 `enable_raw_mode` 才报错，证明 scan+分组建行链路无 panic。

**可行性发现（实现期捕获）**：
1. **Debian 改名**：bookworm 把 `bat`->`batcat`、`fd`->`fdfind`（避命名冲突），L2 片段须 `ln -sf` 软链成常规名--已修。
2. **别名 vs 二进制的交付差异（核心发现）**：二进制装 `/usr/local/bin` 在所有 shell PATH 上 -> 全终端可用；但**别名/函数是 shell 内部**，靠 source 加载，`/etc/profile.d` 只被 **login shell** source。实测三类终端：
   - **AIO 终端面板**（`app/src/pty.rs` `/bin/bash -l`，交互 login）：别名+工具均可用 ✅
   - **code-server 终端**（VS Code 默认 = 非login 交互 shell，source `/etc/bash.bashrc`+`~/.bashrc`，**不碰 `/etc/profile.d`**）：工具可用、别名**默认缺失** ❌
   - **vnc**（`FROM debian:bookworm-slim`，commit b168f77 已解耦不继承 base；且只跑 chromium/openbox/Xvnc/websockify、无终端模拟器）：不适用，本就不在 vnc 敲 shell。
3. **别名覆盖修法**：Debian `/etc/profile` 第 15-16 行已 source `/etc/bash.bashrc`。故片段除 `/etc/profile.d/aio-shell.sh`（login shell 用）外，再**幂等 append 一行 `[ -f /etc/profile.d/aio-shell.sh ] && . ...` 到 `/etc/bash.bashrc`**（在 `[ -z "$PS1" ] && return` 交互 guard 之后）-> login(经 profile->bash.bashrc) 与非login 交互 shell **共用一个别名文件**。复测：`bash -lc`/`bash -ic`/`bash -lic` 三态 `alias ll` 均定义、`ll` 实跑均成功。别名幂等，login shell 双 source 无害。
4. **code-server `--init-file` 专项验证（关键）**：code-server 的 bash shell-integration 用 `--init-file <shellIntegration-bash.sh>` 注入，其非login 分支只 source `~/.bashrc`（不碰 profile.d）。一度担心 `--init-file` 会 bypass `/etc/bash.bashrc` 导致修法在 code-server 失效。**实测推翻该担心**：Debian bash 即使带 `--init-file`，交互 shell **仍 source `/etc/bash.bashrc`**（`--init-file` 只替换 `~/.bashrc`，不替换系统 `/etc/bash.bashrc`）。用 code-server 真实脚本 `shellIntegration-bash.sh` 精确复现：非login 与 login 两态 `alias ll` 均定义、`ll` 实跑成功；反证（删 `/etc/bash.bashrc` 的 source 行）后别名消失，证明别名经此而来。**并在 named volume 挂 `/home/gem`（卷里 `~/.bashrc` 为默认 skel 无别名）下复测仍成立**--因修法落 `/etc`（系统路径，不被 `aio_workspace` 卷遮盖），与 `~/.bashrc` 无关。故 AIO 终端面板 + code-server 终端别名均可用。
5. **测试方法陷阱**：`type ll` 在非交互 shell 因 `expand_aliases` 关闭而不报别名（不代表未定义）；可靠判据是 `alias ll`（查定义表，不受 expand_aliases 影响）或在交互 shell 实跑。另：code-server 终端输出带 OSC 633 转义前缀，grep 过滤时勿误删别名行。
6. **opencode 与 Web 面板的耦合（Q4=A 的前置发现）**：opencode 不是单纯二进制，而是 `app/services.toml` 注册的 `type=agent` 服务（`enable="ENABLE_OPENCODE"`、`cmd="opencode"`）。`app/src/config.rs::is_agent_enabled` **只读环境变量** `ENABLE_OPENCODE`（compose 硬编码 `true`），不查二进制；前端按 manifest 渲染面板，点击 -> `WS /api/term/ws?cmd=opencode` -> `spawn_pty` -> `/bin/bash -l -c opencode`。故若 opencode 移 L4 且未勾选：二进制不在（app `FROM sandbox-base` 继承），但 `ENABLE_OPENCODE=true` 仍显示面板 -> 点开 `opencode: command not found` 后关闭（死面板，非崩溃）。根因：二进制在不在（信号A）与面板显不显示（信号B=env）被解耦。
7. **用户未来愿景（L4 终态，本 MVP 不做）**：运行时自动检测 L4 装了哪些 AI agent -> Web 动态出按钮 -> 点按钮即开对应界面。这天然涵盖「把 opencode 移到 L4 + app 侧 command_exists 耦合」--作为后续一个完整特性，MVP 阶段不动 app/web。

## Open Questions（恢复 brainstorm 后继续）

- ~~Q3~~ **已定=层序**：gen 改按 `(category_rank, id)` 拼装，生成 Dockerfile.base 读作 head(L1)->L2->L3->L4->tail，与 TUI 分组一致。已实现：`gen.rs` 加纯函数 `sort_by_layer` + 2 单测（层序、未知层排末尾），`cargo test` 8/8 过；启用 rust+shell-utils 验证 shell-utils(L2) 排在 rust(L3) 前。**边界确认：层序仅用于可读性，片段是独立 RUN 层无 build 期依赖，不引入依赖图。**
- ~~Q4~~ **已定+已落地=(a) 现在就移、保持启用**：opencode 从 `Dockerfile.base.head` 迁到 `scenarios/opencode/`（`category="app"`，L4 首个示例，用真 AI agent 验证 L4 路径）。head 删 opencode ARG+安装块、注释同步；用户本地 `.aio/enabled.toml` 加 `opencode` 保持启用（当前沙箱行为不变）。验证：`[opencode]` 单建 sandbox-base，`opencode --version` -> 1.18.7、`/usr/local/bin/opencode` 就位；最终 manifest `[opencode,python-dev,rust]` gen 后装配序 python-dev->rust->opencode（L3,L3,L4）。**latent 死面板（未勾选时）作为已知 MVP 限制记录，留给未来「自动检测 L4 AI -> Web 动态按钮」特性修（见发现 6/7），本 MVP 不动 app/web。** README 分层表/场景清单/file-tree 已同步。
- Q5：L1 是否做"只读展示"进 TUI（Q2 选项 B）或拆出"纯便利工具可勾选子类"（Q2 选项 C 子集），还是就此锁定 L1 全常驻？
- Q6：迁移--已提交的 `Dockerfile.base` 此前未烘场景（gen 后新增 python-dev+rust 片段），是否随本任务一并提交生成产物？

