# 架构总览 / Architecture

[← 返回首页](Home)

本页描述 AIO devbox 的整体架构:各容器的职责与拓扑关系、共享基础镜像的
派生结构、网络命名空间与端口约定、工作区卷的数据持久化方式。

## 容器拓扑

一条 `make up` 拉起的核心栈(`docker-compose.yml`):

| 容器 | 镜像 | 职责 | 启动条件 |
|---|---|---|---|
| `gateway` | `caddy:2` | HTTP basic auth + 反向代理,唯一对外端口 `8080` | 总是 |
| `app` | `sandbox-app`(Rust axum + React SPA) | 工作区后端:按钮清单 manifest、pty 桥、`/preview/<port>` 反代、pi-web 自启 | 总是 |
| `code-server` | `sandbox-code-server` | 浏览器版 VSCode,监听 8200 | profile `code-server` |
| `vnc` | `sandbox-vnc`(Debian slim + Chromium + noVNC) | 浏览器内 Chromium,监听 6080 | profile `vnc` |
| `base` | `sandbox-base` | 不是运行容器——只是 `make build` 的构建入口 | profile `build` |

`code-server` 与 `vnc` 都要用 `make up PROFILES="code-server vnc"`(或
`make config` 之外的单开关 `make up PROFILES=code-server`)显式拉起:容器没跑,
UI 侧的 TCP 探测就探不到,按钮自动隐藏——**不存在死面板**。

## 镜像派生关系

```
Dockerfile.base.head          ┐
scenarios/*/fragment.Dockerfile ─→ aio-config gen ─→ Dockerfile.base ─→ sandbox-base
Dockerfile.base.tail          ┘
                                                    ├─→ sandbox-app         (FROM sandbox-base,多阶段 + web-builder)
                                                    └─→ sandbox-code-server (FROM sandbox-base)
sandbox-vnc                                            (FROM debian:bookworm-slim,刻意解耦:纯浏览器面,不背开发工具链)
```

- 场景烘进 `sandbox-base` 后,app / code-server 自动继承;vnc 与场景变更
  **互不触发重建**。
- 想改工具链不需要动任何服务 Dockerfile——加/改场景后 `make build` 即可
  (见 [场景配置](Scenarios))。
- ⚠️ `docker compose up --build` 对**运行中**的服务不会重建:改了 Dockerfile
  后要显式 `make build` + `docker compose up -d --force-recreate <service>`。

## 共享网络命名空间与端口约定

`code-server` 和 `vnc` 都用 `network_mode: "service:app"` 与 app 共享网络栈
(loopback + 端口空间都是 app 的):

- 侧车容器在 sandbox-net 上没有自己的 DNS 名,统一以 `app:PORT` 被访问
  (manifest 靠 TCP 探测 `app:8200` / `app:6080` 决定按钮显隐);
- 三方共享 loopback,所以 workbench 终端、code-server 终端、Chromium 看到的
  `localhost:PORT` 是**同一个端口空间**——dev server 任意一侧起、任意一侧
  预览,Chromium 的 HTTPS-first 强制升级对 loopback 豁免;
- 保留端口:`8088`(app)、`8200`(code-server)、`6080`(noVNC)、
  `5900`(VNC raw)、`30141`(pi-web,宿主侧可经 `PI_WEB_HOST_PORT` 改发)。

⚠️ **不要单独 `docker restart aio-app-1`**:docker 会重建 app 的网络命名空间,
单独重启后侧车还指向死掉的旧 netns,网络全断。恢复方式 = 一起重启侧车;
日常操作走 `make restart` / `make up`。

## 工作区卷与数据存活

命名卷 `workspace` 挂在所有业务容器的 `/root`:

- **扛过重建**(`down` / `up` / recreate):项目、配置、`~/.local/bin` 工具、
  Chromium profile、code-server 用户设置、pi 登记数据都在卷上;
- **卷遮盖铁律**:镜像里凡是烘在 `/root` 下的东西都会被卷盖掉而失效。所以
  所有场景一律装**系统路径**(`/opt`、`/usr/local`、`/etc/profile.d`);
- 容器**可写层**的运行时改动(不在卷上的)recreate 即丢——运行时试装工具
  只当试用,要留就进场景或 `~/.local/bin`。

## 网关鉴权与反向代理路径

Caddy(Caddyfile 挂载进 gateway)在 basic auth 之后按序匹配:

| 路径 | 后端 | 说明 |
|---|---|---|
| `/code-server/*` | `app:8200` | `handle_path` 剥前缀;code-server 全相对资源 URL,天然兼容子路径 |
| `/vnc/*` | `app:6080` | `handle_path` 剥前缀;noVNC 的 WebSocket 路径需 `path=vnc/websockify` 特判 |
| 其余(catch-all) | `app:8088` | React 工作区 SPA + axum API |

密码由 `make hash` 生成(Caddyfile 内嵌哈希,`make up` 前置校验 `ensure-hash`)。
pi-web 因 Next.js 根绝对资源路径走不了子路径,不进网关,由 compose 把
`30141` 直接发布到宿主(可用 `PI_WEB_HOST_PORT` 换端口)。

[场景配置](Scenarios) 展开工具链预置的完整工作流,离线运行见
[离线分发](Offline-Bundle),部署中的具体坑见 [常见问题](FAQ)。
