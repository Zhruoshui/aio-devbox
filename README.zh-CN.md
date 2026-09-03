[English](README.md) · [中文](README.zh-CN.md)

# AIO 开发沙箱

一个自托管的 all-in-one 远程开发环境。一条 `docker compose up` 拉起 Caddy 网关
和若干可插拔服务容器,浏览器里呈现一个带可折叠侧边栏按钮的工作区:浏览器版
VSCode(code-server)、VNC 里的 Chromium、终端、按需打开的 AI agent TUI
(opencode、pi),以及一个模型配置页——每个按钮点开一个标签页,只有镜像里
真实存在的能力才会有按钮。个人项目。

各种工具链(Node、Python、Rust、Go……)是**构建期场景预置**——你在 TUI 里勾选
(并选 Node/Python 版本),它们被烘进一个所有开发容器共享的 `sandbox-base`
镜像。整套栈**支持离线**:联网机构建,打包镜像运走,气隙环境也能跑。

## 特性

- **一条命令出浏览器 IDE。** `make up` → 打开 `http://localhost:8080`(HTTP
  basic auth)。左侧可折叠侧边栏列出按钮,每次点击在主区启动一个**新实例**标签页
  (终端默认打开),标签页可拖拽拆分/平铺(golden-layout),也可用 tab 上的 ✕ 关闭。
- **可插拔按钮,自动探测——三种类型。**
  - `web`(code-server、VNC):由 compose profile 控制——容器没跑就没有按钮
    (app 侧 TCP 探测)。
  - `agent`(终端、opencode、pi):只在命令真实存在于登录 shell PATH 时才出现,
    点击即用 xterm.js + WebSocket pty 桥在 `app` 容器内启动(可多开)。
  - `page`(模型配置):原生 React 面板,始终启用——为烘进镜像的 agent CLI
    提供统一的 provider/模型配置(编辑配置、从 pi 导入、按 agent 应用、用量统计)。

  没有死面板。
- **自定义按钮。** 侧边栏底部 `+` 表单可注册"终端+命令"按钮(经
  `POST/DELETE /api/buttons` 持久化到工作区卷的 `/root/.aio/buttons.toml`),
  扛得过容器重建。也可注册 **web 型按钮**,指向你在终端里起的任意 dev server
  端口(vite、`python -m http.server`……):端口有监听时按钮出现,点击经 app 的
  `/preview/<port>/` 反代在 iframe 中打开(WebSocket / SSE 友好)。
- **构建期场景预置。** 一个 Rust TUI(`aio-config`)按层分组列出场景;勾选结果
  被组装进 `Dockerfile.base`(生成物,不入 git)并构建进 `sandbox-base`。
  工具链不需要各自的 Dockerfile。
- **基础运行时可版本化。** Node 与 CPython 是 `always_on` 场景,带版本下拉——
  选的是版本,不是装不装。
- **扛容器重建。** 工作区是挂在 `/root` 的命名卷;运行时用户数据(项目、配置、
  `~/.local/bin` 工具)都在卷上,扛过 `down`/`up`。注意:已部署沙箱里运行时
  `mise use` 落容器可写层,recreate 即丢(已知取舍——离线整目录搬迁是受支持
  的路径,见 `docs/offline-tool-install.md` §14)。
- **支持离线。** 联网机 `make save` 打包(镜像 + `.env` + 网关哈希 + 场景选择),
  离线机 `make load` 恢复(或裸 `docker save`/`load`),`make up NOBUILD=1` 运行。
  完整的离线补装手册见 [`docs/offline-install-guide.md`](docs/offline-install-guide.md)。

## 快速开始

```sh
make hash                              # 网关密码(默认 admin)
make config                            # (可选)TUI:勾场景 + 选 Node/Python 版本
make up PROFILES="code-server vnc"      # 带 Web 按钮启动(终端始终启用)
# → 打开 http://localhost:8080   (admin / admin)
```

不带 `PROFILES` 时,只启动常驻服务(`gateway` + `app`),侧边栏显示终端和模型配置
按钮(及已烘进的 opencode / pi 等 agent TUI);加上 `code-server` / `vnc` profile
才会点亮浏览器 IDE 和 Chromium 按钮。

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
                          │ :30141 ← │ (pi-web,iframe 面板,端口直发宿主)
                          └────┬─────┘          │                │
                               │                ▼                ▼
                 /api/term/ws  │ pty (root)  ┌────────────────────────────┐
                 /api/manifest │             │  aio_workspace  →  /root   │
                 /api/models/* │             │  (root,uid 0)共享卷,        │
                 /api/buttons  │────────────►│  三个运行时容器都挂载        │
                               │             └────────────────────────────┘
```

| 容器 | 镜像 | 职责 |
|---|---|---|
| `gateway` | `caddy:2` | HTTP basic auth + 反向代理到 `app`、`code-server`、`vnc`,也转发 WS 升级。 |
| `app` | `sandbox-app`(构建) | Axum 服务:托管 React SPA、`GET /api/manifest`(哪些按钮在线)、`/api/term/ws` pty WebSocket 桥、`POST/DELETE /api/buttons`(用户自注册按钮)、`/api/models/*`(模型配置页)、`/api/stats`、`/preview/<port>/` dev server 动态反代。烘进 pi-web 时由 entrypoint 自启在 `:30141`。`FROM sandbox-base`。 |
| `code-server` | `sandbox-code-server`(构建) | 浏览器版 VSCode。profile 控制,TCP 探测 `app:8200` 自动探测。`FROM sandbox-base`。 |
| `vnc` | `sandbox-vnc`(构建) | Xvnc + Chromium + noVNC Web 客户端。profile 控制,TCP 探测 `app:6080` 自动探测。`FROM debian:bookworm-slim`(与 `sandbox-base` 解耦)。`shm_size 2gb` 供 Chromium。 |
| `base` | `sandbox-base`(构建) | 共享基础镜像。挂在 `build` profile 下,故**绝不**作为运行时容器启动。 |

**共享网络命名空间:**`code-server` 与 `vnc` 通过 `network_mode:
"service:app"` 加入 app 的网络栈(它们在 sandbox-net 上的独立 DNS 名已
不存在,共享栈上的一切都以 `app:PORT` 访问)。因此 VNC 面板里的 Chromium 用
`http://localhost:<port>` 即可访问工作台或 code-server 终端里启动的 dev
server(同一回环,不触发 HTTPS-first 升级)。共享 netns 上的保留端口:
`8088`(axum)、`8200`(code-server)、`6080`(websockify)、`5900`(Xvnc,
回环)、`30141`(pi-web,直发宿主)——dev server 请避开这些端口。

**构建顺序很重要:** `app` 和 `code-server` 都是 `FROM sandbox-base`,所以必须先
构建并打好 `sandbox-base` 的 tag。Makefile 已处理(`make up` → `build-base` →
`compose up --build`)。

## 场景预置

开发环境按**分层模型**组织。每个场景都是一个构建期 Dockerfile 片段,烘进
`sandbox-base`,带一个 `category` 字段让 TUI 按层分组:

| 层 | `category` | 放什么 | 可勾选? |
|---|---|---|---|
| L1 OS 包 | `os` | 非版本化基础设施(apt、ca-certs、build-essential、字体);**版本化运行时 Node + Python** 作 `always_on` 场景 | 基础设施:硬编码;node/python:可选版本、始终启用 |
| L2 Shell 便利 | `shell` | CLI 工具(fzf / rg / bat / fd) | 是 |
| L3 语言工具链 | `lang` | mise(rust + go + uv + ruff + opencode 全家桶)/ c23 | 是 |
| L4 应用 | `app` | CLI 应用 / AI agent(opencode、pi、pi-web) | 是 |
| L5 外部服务 | `service` | _(未来,尚未实现)_ | — |

L1 的**非版本化基础设施**(HTTPS apt 源、ca-certs 自举、build-essential)硬编码在
`Dockerfile.base.head`,不进 TUI——它是所有 `FROM sandbox-base` 服务继承的地基。
**版本化运行时** Node + Python 是 `always_on` 场景:始终烘进(code-server 和
app 的 web-builder 依赖 Node),TUI 里显示为锁定行 `[*]`,版本 `[label]` 用
**左/右方向键**循环。L2–L4 是普通可勾选偏好。

当前场景清单(一律装**系统路径**——`/opt`、`/usr/local`、`/etc/profile.d`——
绝不装 `/root/*`,共享工作区卷会遮盖它):

| 场景 | 层 | `always_on` | 版本 | 装到 |
|---|---|---|---|---|
| `node` | L1 `os` | ✓ | 22.23.2 *(默认)* / 22.11.0 / 20.18.0 / 18.20.4 | nodejs.org tarball → `/usr/local` |
| `python` | L1 `os` | ✓ | 3.12.7 *(默认)* / 3.13.0 / 3.11.10 | python-build-standalone → `/usr/local` |
| `fonts` | L1 `os` | — | — | Maple Mono NF CN(等宽 + Nerd Font 图标 + 中文,~78MB)→ `/usr/local/share/fonts`,经 `/etc/fonts/local.conf` 钉为 mono/sans/serif 默认;修复服务端渲染豆腐块 |
| `shell-utils` | L2 `shell` | — | — | fzf / ripgrep / bat / fd → `/usr/local/bin`(Debian `bat`→`batcat`、`fd`→`fdfind` 软链) |
| `c23` | L3 `lang` | — | — | clang-22(apt.llvm.org,完整 C23)+ 复用 gcc-12 + gdb / cmake / ninja / valgrind / cppcheck / strace;无版本后缀软链 → `/usr/local/bin` |
| `mise` | L3 `lang` | — | — | mise(L3 工具链统一管理器)把 rust + go + uv + ruff + opencode 一并烘到 `/opt/mise`(五工具全家桶,~1.5GB);版本升级 = 改 fragment ARG 块;可见性 = ENV shims PATH + `/etc/profile.d/mise.sh` activate |
| `opencode` | L4 `app` | — | — | (由 `mise` 场景附带烘焙)opencode AI agent CLI,经 mise shims 提供。侧边栏按钮仅在烘进镜像时出现(命令存在探测)。 |
| `pi` | L4 `app` | — | — | pi coding agent → `/usr/local/bin`;扩展烘到 `/opt/pi-extensions`,在终端跑一次 `aio-pi-extensions` 即离线登记进 `~/.pi`(卷) |
| `pi-web` | L4 `app` | — | — | pi 的 Web UI(npm 全局;需 node ≥ 22.19);由 app entrypoint 自启在 `:30141`,iframe 面板内嵌,端口直发(Next.js 根绝对资源路径,走不了网关子路径) |

**工作流。** `make config` 打开 TUI(ratatui):场景按层分组;**空格**勾选,
**左/右方向键**循环 `always_on` 版本,`s` 保存到 `.aio/enabled.toml`。随后
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

**预设与通配符。** `.aio/presets/{minimal,full}.toml` 是现成选区:`minimal` =
仅 always_on 基线(`scenarios = []`),`full` = `scenarios = ["*"]`,由 `gen`
展开为所有发现的非 `always_on` 场景(新场景自动纳入)。CI 把预设拷成
`.aio/enabled.toml` 构建两个 GHCR 变体;通配符必须独占数组(`["*", "mise"]`
是错误)。

**新增场景** = 放 `scenarios/<id>/{scenario.toml,fragment.Dockerfile}`,在
`scenario.toml` 里设 `category`。版本化场景再加 `always_on`(若始终烘进)、
`default_version`、`[[versions]]` 数组(每项:`label` 用于下拉 + 其余 key 作
`{{key}}` 占位符替换进片段)。默认值:`category="lang"`、`always_on=false`、无版本——
配置器无需改动。改了选择就要重建镜像(离线路径不变)。

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
| `make save` / `make load` | 离线 bundle:`save` 把镜像 + `.env` + 网关哈希 + 场景选择打包进 `aio-offline-bundle/`;`load` 在离线机恢复。 |
| `make clean` | 破坏性:`down -v` + 删已构建镜像。 |
| `make pull [VARIANT=…]` | 从 GHCR 拉预构建镜像 + retag 为本地 compose 名(见下)。 |

内部辅助目标:`build-config`(构建 `aio-config` 镜像)、`ensure-hash`(缺省时写
默认密码哈希;由 `up` / `pull` 调用)。

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

```sh
# 联网机
make save                                  # → aio-offline-bundle/:images.tar + env + hash + enabled.toml
# 离线机(bundle 拷过去)
make load                                  # 恢复镜像 + .env + 哈希 + 场景选择
make up NOBUILD=1 PROFILES="code-server vnc"
```

烘了 `pi` 场景的话,首次启动后在终端跑一次 `aio-pi-extensions`,把烘好的扩展
登记进 `~/.pi`。`aio-config` 镜像构建时也要从 crates.io 拉 crate,故同样
联网构建、离线载入。

完整手册——如何给运行中的离线栈补装任意工具/包而不重建、不联网(7 个实测配方:
静态二进制、npm 全局包、apt deb、cargo crate、rust 工具链、python+uv、脚本)——见
[`docs/offline-install-guide.md`](docs/offline-install-guide.md)。

## 预构建镜像安装(GitHub Actions)

不想本地构建镜像?每次推 `main`(以及每个 `v*` tag)都会由 GitHub Actions 构建并
发布到 GitHub Container Registry(GHCR)。基础派生镜像有两个变体,外加单份
`sandbox-vnc`:

- `minimal`——纯 always_on 基线(Node + Python),别无其他。
- `full`——全部场景片段都烘进去(mise [rust/go/uv/ruff/opencode] / c23 / pi / …)。

```sh
make pull VARIANT=full           # 拉取并 retag 为本地名(默认 full)
make up NOBUILD=1 PROFILES="code-server vnc"   # 不构建直接启动
# → 打开 http://localhost:8080   (admin / admin)
```

`make pull` 以 `:minimal` 或 `:full` 拉取 `sandbox-base` / `sandbox-app` /
`sandbox-code-server`,外加 `sandbox-vnc:latest`,retag 成 compose 本地名,并备齐
栈启动所需的两个 gitignore 主机文件(`.env` 从示例复制,以及默认密码 `admin` 的
网关哈希)。它绝不碰 `.aio/enabled.toml`——纯消费者无需关心场景选择,`make up
NOBUILD=1` 也不跑 `gen`。

用 `REGISTRY_PREFIX` 指定你的镜像仓库(默认即本仓库的 GHCR 命名空间
`ghcr.io/zhruoshui`,从 fork 拉取时覆写),用 `VARIANT=minimal` 拉更精简的集合。
如果机器完全无法访问镜像仓库,走上面的离线路径(`make save` / `make load`)。

## 项目结构

```
Dockerfile.base          sandbox-base 镜像(由 `make gen` 生成,不入 git)
Dockerfile.base.head     sandbox-base 头(root 自举:apt;无语言运行时)
Dockerfile.base.tail     sandbox-base 尾(USER root + WORKDIR /root)
scenarios/               场景库,按 category 分层;<id>/{scenario.toml,fragment.Dockerfile}
config/                  aio-config crate(Rust):TUI 勾选器 + Dockerfile.base 生成器
app/                     axum 应用(Cargo.toml、src/、Dockerfile、services.toml)
  └ services.toml        内置工作区按钮(id/type/target/url/label/cmd)
web/                     React SPA(Vite + TS + 侧边栏/标签栈 + xterm.js),烘进 app 镜像
gateway/                 Caddyfile + entrypoint.sh(+ secrets/hash,生成)
vnc/                     Xvnc + Chromium + noVNC(FROM debian:bookworm-slim)
code-server/             浏览器版 VSCode 镜像(FROM sandbox-base)
docker-compose.yml       gateway + app + code-server + vnc + base(build profile)
Makefile                 config / gen / build-base / up / hash / save / load / pull / clean
.env / .env.example      SANDBOX_USER(哈希是生成的,不经 env 传递)
docs/                    offline-install-guide.md(+ offline-tool-install.md 实测记录)
.aio/enabled.toml        场景选择(make config 写,gen 读)
.aio/presets/            minimal.toml / full.toml——CI 预设(`["*"]` 通配符 = 全选)
aio-offline-bundle/      `make save` 的输出(gitignored)
```

## 状态

分阶段构建。MVP 已完成:gateway + app(axum + React SPA)+ code-server + vnc,带四
层与版本化 L1 运行时的场景预置系统、离线支持,侧边栏按钮化工作区(web/agent/page
三类自动探测按钮、用户自注册 agent/web 按钮 + dev server 端口预览、统一模型配置),
以及 pi / pi-web agent 栈。
尚未做:按需 TUI 按钮之外的 L5 外部服务、终端多实例。

### dev server 预览(`/preview/<port>/`)

注册一个 web 型按钮(侧边栏 `+` → 选「Web 端口预览」,填 dev server 监听的端口),
点击即可在 iframe 中打开。app 把 `/preview/<port>/*` 反代到共享网络命名空间里的
`127.0.0.1:<port>`,所以工作台 / code-server 任一终端里起的服务都可达(含只绑
loopback 的)。端口有监听时按钮出现(TCP 探活,与内置按钮同语义),无监听时隐藏。
WebSocket(vite HMR)与 SSE 流原样透传、不缓冲。

两个已知边界:

- **根绝对资源 URL 在任何子路径下都会断。** 输出 `/_next/...` 风格 URL 的应用
  无法挂在 `/preview/<port>/` 下(pi-web 因此需要独立源)。反代不做 HTML 改写。
- **vite 需要两行配置**才能跑在子路径下 —— `vite.config.ts`:
  ```ts
  export default defineConfig({
    base: '/preview/5173/',
    server: { hmr: { path: '/preview/5173/' } },
  });
  ```
  输出纯相对 URL 的服务(`python -m http.server`、多数静态预览)零配置可用。
