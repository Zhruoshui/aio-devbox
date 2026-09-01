# Scenario-based preset dev environment profiles

## Goal

让用户在**部署前**用 TUI(Rust ratatui)勾选要启用的开发场景(python / rust / …),由**构建期**把该场景的标准开发链路烘进 `sandbox-base` 镜像(经 `Dockerfile.base`),`make up` 起来后无需登录手动补装。改选 = 重跑 TUI + 重建镜像(用户明确接受重建)。

## Background(代码已确认的事实)

- AIO 沙箱 = docker compose 栈:`gateway`(caddy basicauth)+ `app`(axum Rust + React SPA)+ `code-server` + `vnc`,均 `FROM sandbox-base`。
- `sandbox-base`(`Dockerfile.base`):`FROM debian:bookworm-slim`。现有内容(已读源确认):强制 HTTPS apt 源(sed)、ca-certificates 自举、apt 装 curl/git/gnupg2/xz/python3/pip/venv/build-essential/pkg-config/libssl-dev/locales/tzdata/sudo;node 20 从 `nodejs.org` tarball 装 `/usr/local`;opencode 从 GitHub release 装 `/usr/local/bin`;建用户 `gem`(uid 1000),末尾 `USER gem` + `WORKDIR /home/gem`。
- **构建期需联网**:`Dockerfile.base` 的 `RUN apt-get` / `RUN curl nodejs.org|github.com` 在 `docker build` 时执行 -> 离线机无法 build base。既有离线哲学:base 镜像在**联网机**构建 -> `docker save` -> 离线机 `docker load` -> `make up` 只用已 load 镜像(离线机不 build base)。方案 A 与此一致,无新增矛盾。
- 共享卷 `aio_workspace` 挂 app/code-server/vnc 的 `/home/gem`(uid 1000)。**卷遮盖陷阱**:命名卷首次挂载拷贝镜像 `/home/gem` 内容,之后非空不再拷贝 -> 烘进镜像 `/home/gem/*` 的工具首跑可见但升级 stale、语义混乱。故场景工具必须落**系统路径**(`/usr/local`、`/opt`),不落 `/home/gem`。
- 构建编排:`Makefile` `build-base`(`docker build -t sandbox-base -f Dockerfile.base .`)/`build`/`up`。`base` 服务在 `build` profile 下,运行时不启动。`make up` 当前链路:`build-base` -> `ensure-hash` -> `compose up --build`(`up` 在离线机会因 `build-base` 联网失败,需 NOBUILD 旁路,见 design)。
- 离线补装指南(`docs/offline-install-guide.md`,7 类方法 A–G)管**部署后**临时补装到共享卷 `~/.local/bin`;与**部署前**场景预置(本任务)互补,不重叠。方案 A 下,rust 等场景在构建期**在线**安装(联网机有网),**不需要**离线指南方法 D/E(rustup toolchain link / cargo vendor)--那些是为离线运行时补装设计的;离线靠整镜像 save/load。

## 设计转变(相对前版 PRD 的方案 C,已确认改选)

前版选**方案 C**(制品烘进 catalog,运行时 provisioner 解压到共享卷 `~/.local/bin`,改选不重建)。现改选**方案 A**(构建期烘进 `Dockerfile.base`),三点反转:

- **方案 A**:构建期 baking。改选 = 重跑 TUI + 重建 base(+ save/load 到离线机)。自由性下降,但实现大幅简化(无 catalog、无运行时 provisioner、无卷解压)。
- **落点反转**:从共享卷 `~/.local/bin` 改为**镜像系统路径**(`/opt`、`/usr/local`,root 装、`USER gem` 前,必要时 `chown gem`),避开卷遮盖,且 `/usr/local/bin`/`/opt/.../bin` 默认在 PATH,无需 `~/.bashrc` 兜底。运行时补装仍走 `~/.local/bin`(指南既有分工,不变)。
- **新增/勾选都需重建**:但都只动 manifest/片段,不动生成器/TUI 核心。

## Resolved(本轮访谈)

- **Q1-output = manifest + 生成器拼装**:TUI 只写选择清单 `.aio/enabled.toml`(enabled 场景 id 列表);`make build-base` 调生成器,把 `Dockerfile.base.head`(现有精细引导段:HTTPS sed / ca-cert 自举 / node / opencode,原样保留,**TUI 不碰**)+ 已选场景 `fragment.Dockerfile` + `Dockerfile.base.tail`(`USER gem` + `WORKDIR /home/gem`)拼成 `Dockerfile.base`。装配顺序:场景片段以 root 跑(装 `/opt`/`/usr/local`),在 `USER gem` 之前。
- **Q2-gran = 整 bundle**:场景 = 一份固定标准链路(`rust`=rustup stable+rustfmt+clippy(+rust-analyzer);`python-dev`=uv+ruff)。可叠加多个 bundle,bundle 内不拆单工具。一个场景 = 一个片段文件(R5 净)。
- **Q3-scenario-fmt = 裸 Dockerfile 片段 + 旁挂元数据**:每个场景 = `scenarios/<id>/` 目录,含 `fragment.Dockerfile`(字面 RUN/ENV/ARG,生成器只拼接)+ `scenario.toml`(id/显示名/描述,TUI 列表用)。生成器平凡,场景能表达 apt/curl/rustup/cargo vendor 任意多步。错别字在 `docker build` 时报错(无 schema 校验,可接受:片段开发者 authored)。
- **Q4+Q5 = 单 Rust 二进制 tui+gen,同一镜像 aio-config**:一个 Rust 二进制,`tui`(交互勾选)/`gen`(读 enabled.toml + scenarios/* 拼出 Dockerfile.base)两个子命令,一个容器镜像 `aio-config`。`make config`=`docker run --rm -it aio-config tui`;`make build-base`=`docker run --rm aio-config gen` 再 `docker build`。单语言、共享 scenario 发现 + TOML 清单代码、宿主只需 docker。代价:`build-base` 依赖先建 `aio-config` 镜像(`make build-config` 目标搞定)。

## Requirements

- R1:每个场景独立声明(`scenarios/<id>/fragment.Dockerfile` + `scenario.toml`),描述该场景标准开发链路装什么、装到哪个系统路径、构建期在线安装方法(apt / 官方 tarball / rustup 等)。
- R2:宿主机 TUI(ratatui)可自由勾选要启用的场景(可叠加,如 rust + python-dev),写出 `.aio/enabled.toml`。
- R3:`make up`(经 `make build-base` 的 `gen`+`docker build`)按选择把所选场景烘进 `Dockerfile.base` 后构建,首跑即就绪,无需登录后手动补装。
- R4:全程兼容离线部署(base 在联网机 gen+build -> save -> 离线机 load -> `make up NOBUILD=1` 不 build)。
- R5:新增场景 = 加一份 `scenarios/<id>/` + 重建,不改生成器/TUI 核心(开箱即扩展)。
- R6:烘进镜像的工具落**系统路径**(`/opt`、`/usr/local`,root,`USER gem` 前,必要时 `chown gem`),避开卷遮盖;运行时补装仍走 `~/.local/bin`(指南既有分工)。

## Acceptance Criteria

- [ ] `make config` 起 TUI,勾选 rust;`make up` 后在 code-server 内置终端与 AIO 终端面板两路都能 `cargo --version`、`rustc --version`(无需手动补装)。
- [ ] 勾选 rust + python-dev,`make up` 后两路都能 `cargo --version` 与 `uv --version`。
- [ ] 已跑起来后,`make config` 取消 rust、`make build-base`(重建)+ `make up --force-recreate`,rust 工具链从镜像移除,python-dev 不受影响。
- [ ] 离线机(联网机 `docker save` -> 离线机 `docker load`)`make up NOBUILD=1` 可用,全程不联网(离线机不 build base)。
- [ ] 新增 go 场景只需加 `scenarios/go/` + 重建,不动生成器/TUI 主体。

## Out of Scope

- 多用户/多租户(单用户 `gem`)。
- 运行时(系统跑起来后)动态加减场景;入口仍是部署前 TUI。
- 场景运行时自动探测/推荐(先做显式勾选)。
- 场景间冲突检测(MVP 假设场景互不冲突;文档注明)。
