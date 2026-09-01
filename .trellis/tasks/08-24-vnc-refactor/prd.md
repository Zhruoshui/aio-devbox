# VNC 子系统重构（参考 agent-infra/sandbox 架构）

## Goal

参考 agent-infra/sandbox 的浏览器/VNC 子系统架构，重构 AIO 沙箱现有的 VNC 栈，解决
跨容器预览痛点（浏览器里访问开发服务器需要 `http://app:PORT` 且撞 Chromium HTTPS-first
升级），并吸收参考项目的关键能力（localhost 直通、CDP 驱动、剪贴板同步、中文输入法、
浏览器级守护、Host 头端口代理）。

## Background：两套架构的事实（已勘察确认）

### 现状 AIO（多容器，2026-08-24）

- gateway：caddy:2，宿主唯一发布口 8080，全站 basicauth；路由 `/code-server/*`→
  code-server:8200、`/vnc/*`→vnc:6080（均 handle_path 剥前缀）、catch-all→app:8088
  （gateway/Caddyfile）。
- app：Rust axum :8088（工作台 UI + PTY 终端 + buttons），FROM sandbox-base；用户在
  工作台终端里启动开发服务器（绑定 **app 容器** 的 localhost）。
- code-server：:8200，profile 门控；集成终端里也可启动开发服务器（**code-server 容器**
  的 localhost）。
- vnc：FROM debian:bookworm-slim（刻意与 sandbox-base 解耦，省 ~1.3GB、隔离 base 重建），
  bash wait -n 守护 4 进程：Xvnc :99（1280x800，rfb 5900，-SecurityTypes None
  -localhost）、openbox、chromium（debian 版）、websockify+noVNC 1.3.0 :6080
  （vnc/Dockerfile、vnc/entrypoint.sh）。
- 跨容器预览：vnc 里的 Chromium 只能用 `http://app:3000` 这类 sandbox-net DNS 名访问
  app 容器里的开发服务器（docs/offline-tool-install.md:86）；已知痛点：
  - Chromium 151 HTTPS-first 把 `http://app:3000` 强制升级为 https →
    ERR_SSL_PROTOCOL_ERROR（策略/flag 无效）；唯一稳定解 = socat 转 localhost，但
    socat 未烘进镜像（memory: chromium-https-first）。
  - code-server 容器里启动的服务对 vnc 不可达 localhost（设计上 out of scope，见
    docker-compose.yml:50-54 注释）。
- 已知缺陷（memory + vnc/ 代码注释）：
  - stale `/tmp/.X99-lock` 跨 restart 残留 → aio-vnc-1 flapping；建议 tmpfs /tmp（未实施）。
  - 任一子进程退出 → 整容器重启（wait -n 全有或全无）；Chromium 崩溃/被关 = 桌面重来，
    页面丢失（vnc/DEFERRED-chromium-decorations.md 记录了相关尝试与回退）。
  - 无剪贴板同步（autocutsel 缺失）、无中文输入法（fcitx5 缺失）、无 CDP。
- 研究归档：.trellis/tasks/archive/2026-07/07-28-research-aio-architecture/research/
  02-architecture.md 已含参考项目端口表（6080 WS 代理、5900 VNC、9222 CDP、8102
  MCP-chrome-devtools 等）。

### 参考 agent-infra/sandbox（单容器，用户已实地验证）

- 单容器单入口：nginx :8080（宿主只发布 8081）：
  `/vnc/`→noVNC 静态、`/ws`→websocat :6080（WS→TCP 桥到 127.0.0.1:5900）、
  `/jupyter`→8888、`/code-server/`→8200、`/cdp/`→127.0.0.1:9222、
  **Host 头 `^(\d+)-` 正则 → 127.0.0.1:$port**（任意端口代理）。
- 桌面栈：Xvnc（TigerVNC，X server + VNC server 二合一，非 Xvfb+x11vnc）、openbox、
  **autocutsel ×2**（VNC↔X 剪贴板同步）、**fcitx5**（中文输入法）、
  **chrome --remote-debugging-port=9222**（browser-supervisor.py 守护）。
- websocat（rust）替代 websockify 做 WS→TCP 桥。
- 核心价值：浏览器与用户代码同 network namespace → Chromium 打开
  `http://localhost:9999` 即代码监听的 loopback，零穿透。

### 关键差距表

| 维度 | 现状 AIO | 参考 | 差距性质 |
|---|---|---|---|
| 容器拓扑 | app/code-server/vnc 三容器各自 netns | 单容器 | **结构性（核心痛点根源）** |
| 浏览器↔开发服务器 | `http://app:PORT`（撞 HTTPS-first） | `http://localhost:PORT` 直通 | 结构性 |
| 宿主访问任意端口 | `sbx ports` 手动 publish | Host `9999-*` 头代理 | UX |
| 浏览器 CDP | 无 | 9222 + /cdp/ 路由 + MCP | 能力 |
| 剪贴板同步 | 无 | autocutsel ×2 | 能力 |
| 中文输入 | 无（仅显示 CJK） | fcitx5 | 能力 |
| 浏览器守护 | 整容器 wait -n 重启 | browser-supervisor 级重启 | 健壮性 |
| WS 桥 | websockify(python) | websocat(rust) | 次要 |
| stale X lock | 未修（flapping 根因） | -（supervisord 托管） | 缺陷修复 |

## Decisions

- **D1 容器拓扑（2026-08-24，用户拍板）：路线 A —— netns 共享。** vnc 与 code-server
  改用 `network_mode: "service:app"` 加入 app 的网络栈；保留三个独立镜像、独立
  Dockerfile 与 profile 门控。效果：Chromium 的 localhost == 工作台/code-server 终端
  里启动的 dev server 的 localhost（零穿透，HTTPS-first 对 localhost 天然豁免）；
  CDP 9222 对 app 容器内 agent 直通；`http://app:PORT` DNS 写法废止。
  联动改动：Caddyfile 上游 vnc:6080 / code-server:8200 → app:6080 / app:8200；
  config.rs 探测目标与 services.toml target 同步改名；compose 侧边容器去掉
  networks:/expose:；生命周期与 app 绑定（app 重启时侧边容器网络闪断，靠 restart
  策略自愈 —— design.md 验证项）。
- **D2 WS 桥保持 websockify（本期不换 websocat）。** websocat 无法确认在 bookworm
  apt 源内（检索仅命中 Ubuntu universe），换用需引入 GitHub 二进制下载，破坏纯 apt
  离线故事，收益（去 python 依赖）小。参考项目用 websocat 是其单容器镜像构建约束，
  不构成对本项目的强制。

- **D3 能力范围（2026-08-24，用户拍板）：仅拓扑重构。** 五个能力差距
  （autocutsel 剪贴板、fcitx5 中文输入、CDP+/cdp/ 路由、Host 头端口代理、
  browser-supervisor）全部后置为后续独立迭代；stale X lock 修复属缺陷修复，无条件纳入。

## Requirements

- R1 netns 共享：code-server 与 vnc 服务改 `network_mode: "service:app"`，加入 app
  容器网络栈；三个镜像、Dockerfile、profile 门控不变。
- R2 localhost 直通：vnc 内 Chromium 访问 `http://localhost:<port>` 能命中工作台终端
  **或** code-server 集成终端里启动的 dev server（含 pi-web :30141 场景）。
- R3 网关联动：Caddyfile 上游 `vnc:6080`/`code-server:8200` 改 `app:6080`/`app:8200`；
  对外 URL 形状（`/vnc/*`、`/code-server/*`、catch-all、basicauth）不变。
- R4 探测联动：services.toml `target` 与 config.rs 相关引用同步改为 `app:<port>`，
  UI 按钮显隐行为不变。
- R5 stale-lock 修复：vnc 服务 `/tmp` 用 tmpfs（或等价方案），Xvnc stale lock 不再
  跨 restart 残留导致 flapping。
- R6 行为保持：`make up PROFILES=vnc,code-server` 工作流、Chromium profile 持久化
  （workspace 卷）、noVNC 面板 URL 与自动连接参数均与现状一致。

## Acceptance Criteria

- [ ] AC1 `make build && make up PROFILES=vnc,code-server` 后，工作台 UI 的
  code-server / Chromium 按钮均出现且 iframe 可用（=R3/R4）。
- [ ] AC2 工作台终端跑 `python3 -m http.server 9999 --bind 0.0.0.0`，VNC Chromium
  打开 `http://localhost:9999` 返回目录列表（=R2，同时验证 HTTPS-first 不再触发）。
- [ ] AC3 code-server 集成终端跑任意端口 dev server（如 9998），VNC Chromium 打开
  `http://localhost:9998` 可达（=R1/R2 覆盖第二个源容器）。
- [ ] AC4 `docker restart <vnc容器>` 连续 3 次，容器稳定运行不 flapping，
  `/tmp/.X99-lock` 不残留（=R5）。
- [ ] AC5 pi-web 场景下 VNC Chromium 经 `http://localhost:30141` 打开 pi Web（=R2
  实场景回归）。
- [ ] AC6 回滚验证：revert 本任务 commit 后 `make up` 恢复三 netns 旧架构可正常工作。

## Open Questions

- 无阻塞项。design.md 待验证技术点：caddy 对 app:6080/app:8200 的可达性；app 容器
  restart（保 sandbox）与 recreate（新 netns）两种操作下侧边容器的行为差异；共享
  netns 保留端口清单（8088/8200/6080/5900）文档化。

## Out of Scope

- autocutsel 剪贴板同步、fcitx5 中文输入法、CDP 9222 + /cdp/ 路由、Host 头端口代理、
  browser-supervisor（含 DEFERRED-chromium-decorations.md 的关窗丢页/kiosk 化）
  —— 均为后续独立迭代（D3）。
- websocat 替换 websockify（D2）。
- jupyter 服务（参考有、AIO 无此需求）。
