# 场景配置 / Scenarios

[← 返回首页](Home)

本页描述构建期场景预置系统:分层模型如何组织场景、如何用 TUI 勾选工具链、
版本化运行时的选择方式,以及预设与通配符的用法。

## 分层模型(L1–L5)

场景按 `scenario.toml` 里的 `category` 分层,`aio-config gen` 按
`os → shell → lang → app → service` 排序组装进 `Dockerfile.base`:

| 层 | category | 定位 | 当前场景 |
|---|---|---|---|
| L1 OS / 基础 | `os` | 所有容器依赖的地基;版本化运行时 `always_on` | node、python、**mise engine**、fonts |
| L2 Shell 便利 | `shell` | 纯二进制 CLI 便利工具 | 十个独立条目:fzf / ripgrep / bat / fd / eza / zoxide / delta / starship / jq / yq |
| L3 语言工具链 | `lang` | 编译器 / 工具链 / 语言版本管理(**两派**) | **mise 派**:rust / go / uv / ruff;**系统派**:c23 |
| L4 应用 / agent | `app` | 终端里的 CLI 应用 / AI agent | opencode / claude-code / codex / pi / pi-web |
| L5 外部服务 | `service` | 自带端口 + 面板的 Web 服务 | 预留(code-server / vnc 目前走 compose profiles,不是场景) |

> **粒度重构(2026-09-22)。** `mise` 曾是 all-or-nothing 的五工具全家桶,现
> 收窄为 **engine-only**(L1 `always_on`,~30MB,不装任何工具),工具各自成
> 独立场景。每个工具只为勾选的付体积代价。新增工具 = 新增一个
> `scenarios/<id>/` 目录,不需要任何聚合机制——`mise use -g` 是读改写语义,
> 在 Docker 顺序层上天然合并。

## 当前场景清单

一律装**系统路径**(`/opt`、`/usr/local`、`/etc/profile.d`),绝不装 `/root/*`
(共享工作区卷会遮盖,见 [架构总览](Architecture)):

| 场景 | 层 | always_on | 版本 | 说明 |
|---|---|---|---|---|
| `node` | L1 | ✓ | 22.23.2 *(默认)* / 22.11.0 / 20.18.0 / 18.20.4 | nodejs.org tarball → `/usr/local` |
| `python` | L1 | ✓ | 3.12.7 *(默认)* / 3.13.0 / 3.11.10 | python-build-standalone → `/usr/local` |
| `mise` | L1 | ✓ | — | mise **engine 本体**——二进制 + shims + 四个重定向 env + `/etc/profile.d/mise.sh` activate。**不装任何工具**(~30MB);构建期自检断言 `installs/` 为空 |
| `fonts` | L1 | — | — | Maple Mono NF CN(等宽 + Nerd Font + 中文,~78MB)→ `/usr/local/share/fonts`,修复服务端渲染豆腐块 |
| shell 工具 ×10 | L2 | — | 各一个版本 | fzf / ripgrep / bat / fd / eza / zoxide / delta / starship / jq / yq——一工具一场景,`mise use -g` → `/opt/mise/installs`,shim 在 `/opt/mise/shims` |
| `rust` `go` `uv` `ruff` | L3 | — | 各一个版本 | **mise 派**。`rust` 需 table 形式 spec(`profile = "default"`)+ `rustup component add rust-analyzer`(缺后者 shim↔代理死循环) |
| `c23` | L3 | — | — | **系统派**——clang-22(apt.llvm.org,完整 C23)+ gcc-12 + gdb / cmake / ninja / valgrind / cppcheck / strace。留 apt 的原因:mise 的 clang 走 conda 后端(异源 sysroot),且所需 7 个工具不在其 registry |
| `opencode` `claude-code` `codex` | L4 | — | 各一个版本 | mise 派 AI agent CLI。场景 `claude-code` 装出的二进制是 **`claude`** |
| `pi` | L4 | — | — | pi coding agent,**由 mise 管理**(`pi@0.84.2`,版本钉定以匹配 agent-browser 插件基线);扩展烘 `/opt/pi-extensions`,终端跑一次 `aio-pi-extensions` 离线登记。UI 落位在创建向导**服务区**(与 pi-web 级联) |
| `pi-web` | L4 | — | — | pi 的 Web UI(npm 全局);app entrypoint 自启 `:30141`,iframe 内嵌、端口直发。与 `pi` 同处服务区 |

> **注**:`pi` / `pi-web` 自 S1(09-10)起已**不是** `always_on`,而是普通可选
> 场景——mgr 创建向导的服务开关即它们的 UI,开关状态经 `normalize_services`
> 折进 `manifest.scenarios`。它们仍出现在本仓库的默认选区里。

## mise 的关键设计

L3 曾有 5 个手写场景(rust / go / nvm / uv / python-dev),后来全量收编为
**一个** mise 场景;2026-09-22 的粒度重构又把它拆成 **engine(L1)+ 每工具一个
场景**。下面这些坑是两轮重构都保留的铁律:

- **四重定向躲卷遮盖**:`MISE_DATA_DIR` / `MISE_CONFIG_DIR` / `RUSTUP_HOME` /
  `CARGO_HOME` 全部指到 `/opt/mise`(镜像层)。mise 的默认家目录全在 `~` 下,
  只重定向部分会被卷遮盖,运行时 symlink 悬空会触发静默重下 ~1.4GB;
- **可见性双保险**:ENV 通道(`PATH=/opt/mise/shims:$PATH`,容器内所有进程
  继承,覆盖非 login shell)+ `/etc/profile.d/mise.sh` 的
  `eval "$(mise activate bash)"`(补偿 login shell 被 `/etc/profile` 重置 PATH);
- **rust 完整性**:必须显式 `profile = "default"`(否则丢 clippy/rustfmt)+
  单独 `rustup component add rust-analyzer`(缺组件时 shim↔代理死循环);
- **auto_install 关闭**:烘焙期写进 config.toml `[settings]`,离线机缺工具时
  显式报错而非静默 hang;
- **config 可组合**:engine fragment 用 `>` 建立 `/opt/mise/config.toml` 并写
  `[settings]`;各工具 fragment 用 `>>` 追加自己的 `[tools]` 条目。因为
  `mise use -g` 是读改写语义,多个 fragment 在 Docker 顺序层上**天然合并**,
  不需要任何聚合机制或依赖图(实测:三个独立 RUN 层 → `[tools]` 是三者并集);
- **二进制名可能≠场景名**:`ripgrep`→`rg`、`claude-code`→`claude`。自检必须
  用真实二进制名,写错会「构建通过但运行时不可用」;
- **已知取舍**:运行时 `mise use <tool>` 落容器可写层,recreate 即丢;离线
  补装走整目录搬迁配方(`docs/offline-tool-install.md` §14)。
  *(此项正由任务 `09-22-mise-runtime-volume-config` 处理:卷化 `MISE_DATA_DIR`
  + symlink 种子,实测卷仅占 3.7M 即可让用户自装工具跨 recreate 存活。)*

## TUI 勾选工作流

```sh
make config      # TUI:aio-config(ratatui)
make build       # gen 生成 Dockerfile.base → 构建 sandbox-base
make up          # 重建业务容器(会用新镜像)
```

- TUI 里场景按层分组,**空格**勾选;`always_on` 场景是锁定行 `[*]`——
  Node / Python 用**左/右方向键**切版本,pi / pi-web 无版本下拉、纯锁定;
- 结果写入 `.aio/enabled.toml`(gitignored,本机私有);版本清单记在
  `[[versions]]` 段;
- 改完必须 `make build` 重建 `sandbox-base`,再 `make up` 让业务容器用上新
  镜像——**改场景不重建 = 白改**。

## 预设与通配符

`.aio/presets/{minimal,full}.toml` 是现成选区,CI 构建两个 GHCR 变体用:

- `minimal` = `scenarios = []`(仅 always_on 基线:node + python + pi/pi-web);
- `full` = `scenarios = ["*"]`,gen 展开为**所有**发现的非 always_on 场景,
  新场景自动纳入;
- `["*"]` 必须独占数组,`["*", "mise"]` 是错误(gen 会 bail)。

## 新增一个场景

```
scenarios/<id>/
├── scenario.toml         # id(必须=目录名)/ name / description / category
└── fragment.Dockerfile   # 会被夹进 Dockerfile.base,首尾有 # >>> scenario: id >>> 标记
```

四条铁律(违反是场景 bug 的最大来源,详见
`.claude/skills/aio-env-config/references/scenario-authoring.md`):

1. **装系统路径**,绝不装 `/root/*`(卷遮盖);
2. **login shell 可见**:自建 bin 目录用 ENV PATH 或 `/etc/profile.d/*.sh`
   兜住 `bash -l`(AIO 终端面板就是 pty bash -l);
3. fragment 内网络请求一律 `https://`(沙箱网络策略拦 plain HTTP);
4. **版本用 ARG 钉死**;每条安装带构建期自检(`--version` 失败即中止构建)。

写完跑 `make build-base`,再 `docker exec aio-app-1 bash -lc '<tool> --version'`
验证 login shell 可见;代表性 CLI 记得加进 `.github/workflows/images.yml` 的
full 变体 probe 清单。

架构背景见 [架构总览](Architecture),离线机上的场景选择随 bundle 分发,见
[离线分发](Offline-Bundle);相关问题见 [常见问题](FAQ)。
