# Design：VNC netns 共享重构（路线 A，仅拓扑）

对应 prd.md D1/D2/D3。范围 = 拓扑重构 + stale-lock 修复；五个能力差距后置。

## 1. 架构

### 重构前（三 netns）

```
宿主机浏览器 ── http://127.0.0.1:8080 ──► gateway(caddy, sandbox-net)
   ├─ /code-server/* ──► code-server 容器 netns :8200
   ├─ /vnc/*         ──► vnc 容器 netns :6080 (websockify→127.0.0.1:5900 Xvnc)
   └─ /*             ──► app 容器 netns :8088 (axum + 工作台终端)

Chromium(vnc 容器) ──http://app:3000──► app 容器 dev server   ← HTTPS-first 痛点
```

### 重构后（一 netns，三镜像）

```
宿主机浏览器 ── http://127.0.0.1:8080 ──► gateway(caddy, sandbox-net)
   ├─ /code-server/* ──► app:8200 ─┐
   ├─ /vnc/*         ──► app:6080 ─┤  app 容器的网络栈（netns owner）
   └─ /*             ──► app:8088 ─┘  │ :8088 axum（app 容器）
                                      │ :8200 code-server（code-server 容器,
                                      │          network_mode: service:app）
                                      │ :6080 websockify+noVNC（vnc 容器,
                                      │          network_mode: service:app）
                                      │ 127.0.0.1:5900 Xvnc（vnc 容器内）
                                      └ 任意 :<port> 工作台/code-server 终端 dev server

Chromium(vnc 容器) ──http://localhost:3000──► 同 netns dev server  ✅ 零穿透
```

## 2. Docker 网络语义（本设计的核心契约）

- `network_mode: "service:app"` 使 sidecar 与 app 共享同一 network namespace：
  loopback、eth0、端口空间全共享。与 `networks:`、`ports:`、`expose:` 互斥
  （sidecar 上必须删除这三者）。
- **DNS 名变化**：sidecar 不再有自己的 sandbox-net DNS 名（`code-server`、`vnc`
  消失）；`app` 名保留（app 仍挂 sandbox-net，gateway 经它到达共享栈全部端口）。
  端口发布今后只能挂 app 服务（当前仅 gateway 发布 8080，无影响）。
- **隐式依赖**：compose 为 network_mode 自动建立 depends_on，起序 app → sidecar。
- **绑定地址不变即可达**：websockify 绑 `0.0.0.0:6080`、code-server 现状即可从
  eth0 访问（caddy 经 sandbox-net 能通即证明），改共享 netns 后访问性质相同
  （eth0 → 0.0.0.0 socket）。Xvnc 保持 `-localhost`（127.0.0.1:5900）：websockify
  与它同容器同 netns，回环可达；不对网内暴露，安全边界不变。

## 3. 生命周期矩阵（风险点）

| 操作 | 行为 | 结论 |
|---|---|---|
| `docker restart <sidecar>` | 自身 netns 不变（共享栈仍在）| 安全；X lock 由 tmpfs 解决 |
| `docker restart aio-app-1` | **实测（2026-08-24）：docker 会重建 netns，侧车容器不重启但网络全部失效（manifest 探测转 False）** | **禁止单独执行**；误操作后 `docker restart aio-vnc-1 aio-code-server-1` 即自愈（已实测） |
| `make restart PROFILES=...` | compose 按依赖序整体重启（app 先行）| 安全，实测通过 |
| `make up` / `down && up` | compose 因 network_mode 依赖联动重建全部 | 安全（既有工作流即此）|
| 单独 `up --force-recreate app` 且不 recreate sidecar | sidecar 指向已消失的旧 netns | **禁止**；文档注明永远经 compose 操作 |

## 4. 保留端口清单（共享 netns 冲突面）

`8088`(axum)、`8200`(code-server)、`6080`(websockify)、`5900`(Xvnc, loopback)。
dev server 避开即可（参考项目同样接受此约定，不做技术强制）。写入 README 服务表。

## 5. 变更清单（file-by-file）

| 文件 | 变更 |
|---|---|
| `docker-compose.yml` | code-server/vnc：删 `networks:`、`expose:`，加 `network_mode: "service:app"`；vnc 加 `tmpfs: /tmp`（R5）；注释更新（跨容器预览 out-of-scope 说明作废→localhost 直通；探测名） |
| `gateway/Caddyfile` | 上游 `code-server:8200`→`app:8200`、`vnc:6080`→`app:6080`；头部注释同步 |
| `app/services.toml` | 两个 `target` 改 `app:8200`/`app:6080`；pi-web 注释更新（VNC 侧用 `http://localhost:30141`；`PI_WEB_ALLOWED_HOSTS=app` 保留无害） |
| `app/src/config.rs` | 预计零改动（target 由 services.toml 数据驱动）；仅注释/测试若有硬编码名则同步 |
| `README.md` / `README.zh-CN.md` | 服务表两行：探测名 + netns 共享说明 + 保留端口清单 |
| `docs/offline-tool-install.md` | :86 跨容器预览工作流改为 localhost 写法 |
| `.claude/skills/aio-env-config/references/compose-registry.md`（及 `recipes.md` 相关段） | 新服务接入参考更新为 netns 模式（挂 app 栈、保留端口、Caddyfile 上游写 app:PORT） |
| 不变 | `vnc/Dockerfile`、`vnc/entrypoint.sh`（X lock rm 留作 belt-and-braces）、`web/` 全部、`Makefile`、gateway/entrypoint.sh |

注意：`services.toml` 经 `include_str!` 编译期嵌入 app 镜像，改后必须重建 app
镜像（`make build` 覆盖）。

## 6. 兼容性

- 对外零变化：URL 形状（`/vnc/*`、`/code-server/*`）、basicauth、UI 按钮与 iframe
  路径、profile 工作流、镜像内容、Chromium profile 持久化。
- 旧写法 `http://app:3000` 仍可达（app 的 sandbox-net DNS 名未变），但文档改为
  推荐 `http://localhost:3000`（绕开 HTTPS-first 且通用）。
- code-server 的 `VSCODE_PROXY_URI=/proxy/{port}` 端口预览**意外受益**：code-server
  与 dev server 同 netns 后，其内置端口代理首次真正可用（原 out-of-scope 限制自然
  消除）；compose 注释相应更新。

## 7. 验证计划（对应 prd AC）

- AC1：`make build && make up PROFILES=vnc,code-server`；UI 两按钮出现、iframe 可用；
  `docker exec aio-gateway-1 wget -qO- app:6080` / `app:8200` 探活。
- AC2：工作台终端 `python3 -m http.server 9999`（默认绑 0.0.0.0）；VNC Chromium 开
  `http://localhost:9999` 见目录列表。
- AC3：code-server 集成终端 `python3 -m http.server 9998`；VNC 开 localhost:9998。
- AC4：`docker restart aio-vnc-1` ×3；`docker ps` 无 RestartCount 增长；容器内
  `/tmp/.X99-lock` 属 tmpfs、restart 后不存在。
- AC5：pi-web 场景（`make up PROFILES=vnc` + pi-web 按钮）VNC 开
  `http://localhost:30141`。
- AC6：`git stash` 演练或 revert 后 `make up` 回旧架构可用（提交前在分支上验证）。
- 附加（design §3 矩阵）：`docker restart aio-app-1` 后 sidecar 连通性实测记录。

## 8. 回滚

单 commit 改动，`git revert` 即回三 netns；无镜像层变更（app 镜像重建后回到旧
target 也仅是文案/探测名差异）。tmpfs 无持久化副作用。
