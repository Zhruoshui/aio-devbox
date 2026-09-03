# 场景配置 / Scenarios

[← 返回首页](Home)

本页描述构建期场景预置系统:分层模型如何组织场景、如何用 TUI 勾选工具链、
版本化运行时的选择方式,以及预设与通配符的用法。

## 分层模型(L1–L5)

场景按 `scenario.toml` 里的 `category` 分层,`aio-config gen` 按
`os → shell → lang → app → service` 排序组装进 `Dockerfile.base`:

| 层 | category | 定位 | 当前场景 |
|---|---|---|---|
| L1 OS / 基础 | `os` | 所有容器依赖的地基;版本化运行时 `always_on` | node、python、fonts |
| L2 Shell 便利 | `shell` | 纯二进制 CLI 便利工具 | shell-utils(fzf/rg/bat/fd) |
| L3 语言工具链 | `lang` | 编译器 / 工具链 / 语言版本管理 | **mise**(rust+go+uv+ruff+opencode 五合一)、c23 |
| L4 应用 / agent | `app` | 终端里的 CLI 应用 / AI agent | pi、pi-web(opencode 由 mise 附带烘焙) |
| L5 外部服务 | `service` | 自带端口 + 面板的 Web 服务 | 预留(code-server / vnc 目前走 compose profiles,不是场景) |

## 当前场景清单

一律装**系统路径**(`/opt`、`/usr/local`、`/etc/profile.d`),绝不装 `/root/*`
(共享工作区卷会遮盖,见 [架构总览](Architecture)):

| 场景 | 层 | always_on | 版本 | 说明 |
|---|---|---|---|---|
| `node` | L1 | ✓ | 22.23.2 *(默认)* / 22.11.0 / 20.18.0 / 18.20.4 | nodejs.org tarball → `/usr/local` |
| `python` | L1 | ✓ | 3.12.7 *(默认)* / 3.13.0 / 3.11.10 | python-build-standalone → `/usr/local` |
| `fonts` | L1 | — | — | Maple Mono NF CN(等宽 + Nerd Font + 中文,~78MB)→ `/usr/local/share/fonts`,修复服务端渲染豆腐块 |
| `shell-utils` | L2 | — | — | fzf / ripgrep / bat / fd → `/usr/local/bin`(Debian 改名 `batcat` / `fdfind` 软链回) |
| `mise` | L3 | — | — | mise 统一管理器把 **rust + go + uv + ruff + opencode** 一并烘到 `/opt/mise`(all-or-nothing,~1.5GB);版本升级 = 改 fragment 顶部 ARG 块 |
| `c23` | L3 | — | — | clang-22(apt.llvm.org,完整 C23)+ gcc-12 + gdb / cmake / ninja / valgrind / cppcheck / strace |
| `pi` | L4 | — | — | pi coding agent → `/usr/local/bin`;扩展烘 `/opt/pi-extensions`,终端跑一次 `aio-pi-extensions` 离线登记 |
| `pi-web` | L4 | — | — | pi 的 Web UI(npm 全局);app entrypoint 自启 `:30141`,iframe 内嵌、端口直发 |

## mise 场景的关键设计(2026-09 起 L3 的统一形态)

L3 曾有 5 个手写场景(rust / go / nvm / uv / python-dev),已全量收编为一个
mise 场景。它踩过的坑固化成了铁律:

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
- **已知取舍**:运行时 `mise use <tool>` 落容器可写层,recreate 即丢;离线
  补装走整目录搬迁配方(`docs/offline-tool-install.md` §14)。

## TUI 勾选工作流

```sh
make config      # TUI:aio-config(ratatui)
make build       # gen 生成 Dockerfile.base → 构建 sandbox-base
make up          # 重建业务容器(会用新镜像)
```

- TUI 里场景按层分组,**空格**勾选;Node / Python 是 `always_on` 锁定行
  `[*]`,**左/右方向键**切版本;
- 结果写入 `.aio/enabled.toml`(gitignored,本机私有);版本清单记在
  `[[versions]]` 段;
- 改完必须 `make build` 重建 `sandbox-base`,再 `make up` 让业务容器用上新
  镜像——**改场景不重建 = 白改**。

## 预设与通配符

`.aio/presets/{minimal,full}.toml` 是现成选区,CI 构建两个 GHCR 变体用:

- `minimal` = `scenarios = []`(仅 always_on 基线);
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
