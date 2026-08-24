[English](README.md) · [中文](README.zh-CN.md)

# AIO 开发沙箱

一个自托管的 all-in-one 远程开发环境。一条 `docker compose up` 拉起 Caddy 网关
和若干可插拔服务容器,浏览器里呈现一个带可折叠侧边栏按钮的工作区:浏览器版
VSCode(code-server)、VNC 里的 Chromium、终端、以及按需打开的 opencode 等 AI
agent TUI——每个按钮点开一个标签页,只有镜像里真实存在的能力才会有按钮。个人项目。

各种工具链(Node、Python、Rust、Go、nvm、uv……)是**构建期场景预置**——你在
TUI 里勾选(并选 Node/Python 版本),它们被烘进一个所有开发容器共享的
`sandbox-base` 镜像。整套栈**支持离线**:联网机构建,`docker save`/`load` 镜像,
气隙环境也能跑。

## 特性

- **一条命令出浏览器 IDE。** `make up` → 打开 `http://localhost:8080`(HTTP
  basic auth)。左侧可折叠侧边栏列出按钮,每次点击在主区启动一个**新实例**标签页
  (终端默认打开),标签页可拖拽拆分/平铺(golden-layout),也可用 tab 上的 ✕ 关闭。
- **可插拔按钮,自动探测。** Web 按钮(code-server、VNC)由 compose profile 控制——
  容器没跑就没有按钮。Agent/TUI 按钮(终端、opencode……)只在命令真实存在于登录
  shell PATH 时才出现,点击即用 xterm.js + WebSocket pty 桥在 `app` 容器内启动
  (可多开)。没有死面板。
- **自定义按钮。** 侧边栏底部 `+` 表单可注册“终端+命令”按钮(经
  `POST/DELETE /api/buttons` 持久化到工作区卷的 `/home/gem/.aio/buttons.toml`),
  扛得过容器重建。
- **构建期场景预置。** 一个 Rust TUI(`aio-config`)按层分组列出场景;勾选结果
  被组装进 `Dockerfile.base` 并构建进 `sandbox-base`。工具链不需要各自的 Dockerfile。
- **基础运行时可版本化。** Node(18 / 20 / 22)与 CPython(3.11 / 3.12 / 3.13)是
  `always_on` 场景,带版本下拉——选的是版本,不是装不装。
- **扛容器重建。** 工作区是挂在 `/home/gem` 的命名卷;运行时版本管理器(nvm、uv)
  把运行时装进卷,所以 `nvm install` / `uv python install` 能扛过 `down`/`up`。
- **支持离线。** 联网机构建,`docker save`/`load` 镜像,`make up NOBUILD=1` 运行。
  完整的离线补装手册见 [`docs/offline-install-guide.md`](docs/offline-install-guide.md)。

## 快速开始

```sh
make hash                              # 网关密码(默认 admin)
make config                            # (可选)TUI:勾场景 + 选 Node/Python 版本
make up PROFILES="code-server vnc"      # 带 Web 按钮启动(终端始终启用)
# → 打开 http://localhost:8080   (admin / admin)
```

不带 `PROFILES` 时,只启动常驻服务(`gateway` + `app`),侧边栏显示终端按钮(及
已烘进的 opencode 等 agent TUI);加上 `code-server` / `vnc` profile 才会点亮
浏览器 IDE 和 Chromium 按钮。

```sh
make down                              # 停止(保留镜像和工作区卷)
make logs                              # 跟踪日志
make clean                             # 停止、删卷、删已构建镜像
```

## 架构

```
                         ┌──────────────────────────────────────────────┐
   浏览器 :8080 ───────► │  gateway   (caddy:2)                          │
   admin:admin            │  basicauth + reverse_proxy                    │
                         └──────┬───────────────┬───────────────┬────────┘
                                │ /             │ /code-server/  │ /vnc/
                                ▼               ▼                ▼
                          ┌──────────┐   ┌──────────────┐  ┌────────────┐
                          │ app      │   │ code-server  │  │ vnc       │
                          │ axum +   │   │ :8200        │  │ :6080     │
                          │ React    │   │ profile:     │  │ profile:  │
                          │ SPA      │   │ code-server  │  │ vnc       │
                          │ :8088    │   └──────┬───────┘  └─────┬──────┘
                          └────┬─────┘          │                │
                 /api/term/ws  │ pty (uid 1000)  │                │
                 /api/manifest │                │                │
                               ▼                ▼                ▼
                  ┌──────────────────────────────────────────────────────┐
                  │  共享命名卷  aio_workspace  →  /home/gem              │
                  │  (uid 1000,用户 "gem")—— 三个运行时容器都挂载它        │
                  └──────────────────────────────────────────────────────┘

   仅构建(运行时不启动):  base  →  sandbox-base
        app/Dockerfile  与  code-server/Dockerfile  都是  FROM sandbox-base
        vnc/Dockerfile  是  FROM debian:bookworm-slim(解耦——纯浏览器界面)
```

| 容器 | 镜像 | 职责 |
|---|---|---|
| `gateway` | `caddy:2` | HTTP basic auth + 反向代理到 `app`、`code-server`、`vnc`,也转发 WS 升级。 |
| `app` | `sandbox-app`(构建) | Axum 服务:托管 React SPA、`GET /api/manifest`(哪些按钮在线)、`/api/term/ws` pty WebSocket 桥、`POST/DELETE /api/buttons`(用户自注册按钮,存卷上)。`FROM sandbox-base`。 |
| `code-server` | `sandbox-code-server`(构建) | 浏览器版 VSCode。profile 控制(`--profile code-server`),通过 TCP 探测 `app:8200` 自动探测。`FROM sandbox-base`。 |
| `vnc` | `sandbox-vnc`(构建) | Xvnc + Chromium + noVNC Web 客户端。profile 控制(`--profile vnc`),通过 TCP 探测 `app:6080` 自动探测。`FROM debian:bookworm-slim`(与 `sandbox-base` 解耦)。`shm_size 2gb` 供 Chromium。 |
| `base` | `sandbox-base`(构建) | 共享基础镜像。挂在 `build` profile 下,故**绝不**作为运行时容器启动。 |

**共享网络命名空间:**`code-server` 与 `vnc` 通过 `network_mode:
"service:app"` 加入 app 的网络栈(它们在 sandbox-net 上的独立 DNS 名已
不存在,共享栈上的一切都以 `app:PORT` 访问)。因此 VNC 面板里的 Chromium 用
`http://localhost:<port>` 即可访问工作台或 code-server 终端里启动的 dev
server(同一回环,不触发 HTTPS-first 升级)。共享 netns 上的保留端口:
`8088`(axum)、`8200`(code-server)、`6080`(websockify)、`5900`(Xvnc,
回环)——dev server 请避开这些端口。

**构建顺序很重要:** `app` 和 `code-server` 都是 `FROM sandbox-base`,所以必须先
构建并打好 `sandbox-base` 的 tag。Makefile 已处理(`make up` → `build-base` →
`compose up --build`)。

## 场景预置

开发环境按**分层模型**组织。每个场景都是一个构建期 Dockerfile 片段,烘进
`sandbox-base`,带一个 `category` 字段让 TUI 按层分组:

| 层 | `category` | 放什么 | 可勾选? |
|---|---|---|---|
| L1 OS 包 | `os` | 非版本化基础设施(apt、ca-certs、build-essential、用户 `gem`)在 `Dockerfile.base.head`;**版本化运行时 Node + Python** 作 `always_on` 场景 | 基础设施:硬编码;node/python:可选版本、始终启用 |
| L2 Shell 便利 | `shell` | CLI 工具(fzf / rg / bat / fd) | 是 |
| L3 语言工具链 | `lang` | rust / go / python-dev + 版本管理器 nvm / uv | 是 |
| L4 应用 | `app` | CLI 应用 / AI agent(opencode) | 是 |
| L5 外部服务 | `service` | _(未来,尚未实现)_ | — |

L1 分两部分。**非版本化基础设施**(HTTPS apt 源、ca-certs 自举、build-essential、
用户 `gem`)硬编码在 `Dockerfile.base.head`,不进 TUI——它是所有 `FROM sandbox-base`
服务继承的地基。**版本化运行时** Node + Python 是 `always_on` 场景:始终烘进
(code-server 和 app 的 web-builder 依赖 Node),在 TUI 里显示为锁定行 `[*]`、带
版本 `[label]`,用**左/右方向键**循环切换——选的是版本,不是装不装。L2–L4 是普通
可勾选偏好。

当前场景清单:

| 场景 | 层 | `always_on` | 版本 | 装到 |
|---|---|---|---|---|
| `node` | L1 `os` | ✓ | 20.18.0 / 22.11.0 / 18.20.4 | nodejs.org tarball → `/usr/local` |
| `python` | L1 `os` | ✓ | 3.12.7 / 3.11.10 / 3.13.0 | python-build-standalone → `/usr/local` |
| `shell-utils` | L2 `shell` | — | — | fzf / ripgrep / bat / fd → `/usr/local/bin`(Debian `bat`→`batcat`、`fd`→`fdfind` 软链) |
| `rust` | L3 `lang` | — | — | rustup stable + rustfmt + clippy + rust-analyzer → `/opt/rust`,代理 → `/usr/local/bin` |
| `python-dev` | L3 `lang` | — | — | uv + ruff → `/usr/local/bin`(与 `uv` 重叠——二选一) |
| `go` | L3 `lang` | — | — | Go 1.23 tarball → `/usr/local/go` |
| `nvm` | L3 `lang` | — | — | nvm.sh → `/opt/nvm`;运行时 `NVM_DIR=~/.nvm`(在卷上)使 `nvm install` 扛重建。仅 login shell。 |
| `uv` | L3 `lang` | — | — | uv → `/usr/local/bin`;运行时 `uv python install` → 卷(与 `python-dev` 重叠) |
| `opencode` | L4 `app` | — | — | opencode AI agent CLI → `/usr/local/bin`。侧边栏按钮仅在烘进镜像时出现(命令存在探测),点击在 pty 中启动。 |

**工作流。** `make config` 打开 TUI(ratatui):场景按层分组列出。用**空格**勾选可
选场景;L1 `always_on` 行显示 `[*]`、带版本 `[label]`,用**左/右方向键**循环(不可
取消)。按 `s` 保存选择(场景 id + 版本 label)到 `.aio/enabled.toml`。随后
`make build-base` 运行 `aio-config gen`,把 `Dockerfile.base` 由
`Dockerfile.base.head` + `always_on` 的 L1 运行时 + 已启用的
`scenarios/<id>/fragment.Dockerfile`(按 `category` 再按 id 排序)+
`Dockerfile.base.tail` 组装而成,把所选版本的 `{{version}}`/`{{tag}}` 替换进版本化
片段,再构建 `sandbox-base`。

```sh
make config                       # TUI:勾场景 + 选 L1 版本 → .aio/enabled.toml
make up                           # gen + 构建 sandbox-base + compose up
docker exec aio-app-1 bash -lc 'node --version; python3 --version'   # L1 运行时就绪
```

**新增场景** = 放 `scenarios/<id>/{scenario.toml,fragment.Dockerfile}`,在
`scenario.toml` 里设 `category`。版本化场景再加 `always_on`(若始终烘进)、
`default_version`、`[[versions]]` 数组(每项:`label` 用于下拉 + 其余 key 作
`{{key}}` 占位符替换进片段)。默认值:`category="lang"`、`always_on=false`、无版本——
配置器无需改动。场景工具以 root 装到**系统路径**(`/opt`、`/usr/local`、
`/etc/profile.d`),在 `USER gem` 之前,绝不装 `/home/gem/*`(工作区命名卷会遮盖
它)。改了选择就要重建镜像(`docker save`/`load` 离线路径不变)。

> **重新勾选后要重建。** `make up` 会重建 `sandbox-base` 镜像,但不会重建已运行的
> 容器。改了选择后,跑 `make down && make up`(或
> `docker compose up -d --force-recreate`),让 `app` / `code-server` 用上新基础镜像。

## 配置

### Makefile 目标

| 目标 | 作用 |
|---|---|
| `make config` | 交互式 TUI 勾选 → 写 `.aio/enabled.toml`。 |
| `make gen` | 由 head + 已启用片段 + tail 组装 `Dockerfile.base`。(内部目标,由 `build-base` 调用。) |
| `make build-base` | `gen` + `docker build -t sandbox-base -f Dockerfile.base .` |
| `make build` | `build-base` + `docker compose build` |
| `make up [PROFILES=…]` | `build-base`(或 `NOBUILD=1` 跳过)+ `compose up -d --build` |
| `make hash [PASS=…]` | 为密码 `PASS`(默认 `admin`)生成网关 bcrypt 哈希。 |
| `make down [PROFILES=…]` | 停止栈(保留镜像和工作区卷)。 |
| `make restart` / `make logs` | 重启 / 跟踪日志。 |
| `make clean` | 破坏性:`down -v` + 删已构建镜像。 |

可选服务以空格分隔的 profile 传入:`make up PROFILES="code-server vnc"`。不带
`PROFILES` 时只启动常驻服务(`gateway` + `app`)。`NOBUILD=1` 跳过 `build-base` /
`gen` / `--build`——给离线机用 `docker load` 预构建镜像而非现场构建。

### 鉴权

网关用 Caddy `basicauth`(用户 `admin`,密码默认 `admin`;用户名在 `.env` 里用
`SANDBOX_USER` 设)。bcrypt 哈希含 `$` 字符,经 `env_file` / `environment` 传入时
会被 docker-compose 破坏(它把 env 值里的 `$VAR` 模式当变量插值)。故哈希生成到
`gateway/secrets/hash`(gitignored),经 `gateway/entrypoint.sh` 交给 Caddy——后者在
exec Caddy 前 export 它。Caddyfile 仍按设计用 `{$SANDBOX_PASSWORD_HASH}` 占位符。

```sh
make hash              # 为密码 "admin"(默认)生成哈希
make hash PASS=secret  # 自定义密码
```

## 离线安装

联网机构建,`docker save` 镜像,离线机 `docker load`,再 `make up NOBUILD=1`(跳过
`build-base` / `gen`)。`aio-config` 镜像构建时也要从 crates.io 拉 crate,故同样
联网构建、离线载入。

完整手册——如何给运行中的离线栈补装任意工具/包而不重建、不联网(7 个实测配方:
静态二进制、npm 全局包、apt deb、cargo crate、rust 工具链、python+uv、脚本)——见
[`docs/offline-install-guide.md`](docs/offline-install-guide.md)。

## 项目结构

```
Dockerfile.base          sandbox-base 镜像(生成:head + 场景 + tail)
Dockerfile.base.head     sandbox-base 头(root 自举:apt/用户 gem;无语言运行时)
Dockerfile.base.tail     sandbox-base 尾(USER gem + WORKDIR)
scenarios/               场景库,按 category 分层;<id>/{scenario.toml,fragment.Dockerfile}
config/                  aio-config crate(Rust):TUI 勾选器 + Dockerfile.base 生成器
app/                     axum 应用(Cargo.toml、src/、Dockerfile、services.toml)
  └ services.toml        内置工作区按钮(id/type/target/url/label/cmd)
web/                     React SPA(Vite + TS + 侧边栏/标签栈 + xterm.js),烘进 app 镜像
gateway/                 Caddyfile + entrypoint.sh(+ secrets/hash,生成)
vnc/                     Xvnc + Chromium + noVNC(FROM debian:bookworm-slim)
code-server/             浏览器版 VSCode 镜像(FROM sandbox-base)
docker-compose.yml       gateway + app + code-server + vnc + base(build profile)
Makefile                 build-config / config / gen / build-base / build / up / hash / down / clean
.env / .env.example      SANDBOX_USER(哈希是生成的,不经 env 传递)
docs/                    offline-install-guide.md(+ offline-tool-install.md 实测记录)
.aio/enabled.toml        场景选择(make config 写,gen 读)
```

## 状态

分阶段构建。MVP 已完成:gateway + app(axum + React SPA)+ code-server + vnc,带四
层与版本化 L1 运行时的场景预置系统、离线支持,以及侧边栏按钮化工作区(自动探测
按钮、按需启动 agent TUI、用户自注册按钮)。尚未做:按需 TUI 按钮之外的 L5 外部
服务、自定义 web 型按钮(需跨容器端口预览)、终端多实例。
