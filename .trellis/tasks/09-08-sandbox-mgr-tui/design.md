# Design: 多沙箱 Web 管理界面（sandbox-mgr）

决策依据见 prd.md D1–D12。本文只记录技术设计。

## 1. 总体架构

```
浏览器
  │ *.mgr.localhost (宿主 :80)
  ▼
mgr 栈（独立 compose project `aio-mgr`）
  mgr-gateway (caddy:2)  ── Host 头分流
  mgr-api (Rust axum)    ── 持 Docker socket / 直接调宿主 docker
  mgr-web (静态 SPA, 由 mgr-api 服务)
  mgr-db (SQLite 文件, 挂卷/本地文件)

沙箱 N（每沙箱一个 compose project `sbx-<name>`，mgr 生成的 compose.yml）
  gateway (caddy:2, 无 basicauth, 挂 mgr-net + 自有 net)
  app (axum:8088, pi-web:30141, 挂 mgr-net alias sbx-<name>-piweb)
  code-server / vnc (侧车, network_mode: service:app, profile 门控)
```

**关键网络设计——共享外部网络 `aio-mgr-net`**：

- mgr 启动时 `docker network create aio-mgr-net`（幂等，已存在则跳过）。
- 每个沙箱 compose 中，gateway 与 app 服务额外加入该外部网络：
  `networks: { aio-mgr-net: { aliases: [sbx-<name>] } }`（app 的 alias 为
  `sbx-<name>-piweb`，供总网关按容器 DNS 名转发）。
- 好处：沙箱**不发布任何宿主端口**（替代现状 8080/30141 直发），总网关经
  docker DNS 直达容器；沙箱启停不影响其它沙箱路由。
- 存量栈纳管（R7）：`docker network connect aio-mgr-net <gateway容器>` +
  alias 设置走一次性 `docker network connect --alias`，mgr 登记 adopted 标记。

## 2. mgr 总网关（子域名路由，D8）

`mgr/gateway/Caddyfile` 由 mgr-api **从 SQLite 状态生成**（每沙箱两个 site 块），
写盘后 `docker exec aio-mgr-gateway-1 caddy reload --config ...`（本地裸跑形态则
直接调宿主上的 caddy reload 或重启容器）。无任何 basicauth（D9）。

```caddyfile
# 生成的站点（每沙箱）：
http://sbx-<name>.mgr.localhost {
    reverse_proxy sbx-<name>:8080      # 该沙箱自己的 gateway（caddy）
}
http://sbx-<name>-piweb.mgr.localhost {
    # Host 重写为 PUBLIC 子域名（09-08 实测修正：重写为上游 alias
    # `sbx-<name>-piweb:30141` 会被 pi-web request-security 拒 403——白名单
    # 只接受 *.localhost 后缀与 PI_WEB_ALLOWED_HOSTS 条目，见 mgr/src/caddy.rs）
    reverse_proxy http://sbx-<name>-piweb:30141 {
        header_up Host sbx-<name>-piweb.mgr.localhost
    }
}
```

- 监听 `:80`（本地裸跑直接绑；容器形态 `ports: "80:80"`）。`*.mgr.localhost`
  由系统解析到 127.0.0.1，免 DNS 配置（Linux）。
- 删沙箱 = 重新生成 Caddyfile（去掉该站）+ reload，子域名随即不可达。
- 沙箱内部子路径路由（`/code-server/`、`/vnc/`）完全不动（总网关纯转发）。

### 2.1 pi-web 面板 URL 联动（需改 app）

现状 services.toml piWeb url = `http://{host}:30141/`。多沙箱下沙箱不发布
30141，改由 env 覆盖：

- `app/src/config.rs`：manifest 构建时若 `PI_WEB_URL` env 非空，piWeb 的 url
  直接取该值（跳过 `{host}` 替换）；未设则保持现状——存量/离线流程不受影响。
- mgr 生成的沙箱 compose 给 app 设：
  `PI_WEB_URL=http://sbx-<name>-piweb.mgr.localhost/`。
- `app/entrypoint.sh`：pi-web 自启命令的 `PI_WEB_ALLOWED_HOSTS=app` 改为
  `${PI_WEB_ALLOWED_HOSTS:-app}`；mgr 侧设
  `PI_WEB_ALLOWED_HOSTS=app,sbx-<name>-piweb.mgr.localhost`（网关已重写 Host，
  这是双保险）。

## 3. mgr-api crate 设计

### 3.1 workspace 结构（repo 根 Cargo workspace）

现状 app/、config/ 是两个独立 bin crate。改为根 workspace：

```toml
# 根 Cargo.toml（新增）
[workspace]
members = ["app", "config", "mgr", "aio-models"]
```

- `config/` lib 化：新增 `src/lib.rs`（pub mod scenario/manifest/gen），
  bin 保留（`make config` TUI 兜底不动）。mgr 依赖 `aio-config` path crate
  复用 gen 拼装与场景发现。
- **新 crate `aio-models/`**：从 `app/src/routes/models/{store.rs, catalog.rs}`
  抽出 canonical schema + 读写 + mask/merge/validate（纯逻辑，无 axum 依赖）。
  app 与 mgr 都依赖：mgr 做唯一真相源 CRUD，app 做拉取落盘。render/usage
  留在 app（它们写沙箱内文件、读沙箱内数据）。
- `mgr/` 新 bin crate：axum + rusqlite + tokio。
- 根 workspace 化对现有构建的影响：app/config 的 Cargo.lock 合一到根；
  `app/Dockerfile` 的 COPY 路径不变（workspace 根仍在 repo 根，build 上下文
  已是 repo 根）。**需回归验证 `make build`**。

### 3.2 mgr 目录与数据布局

```
mgr/                    源码 crate
mgr-web/                管理面 React SPA（Vite，复用 web/ 的构建模式与 token）
mgr-data/               运行时状态（gitignore；容器形态挂卷）
  state.db              SQLite
  caddy/Caddyfile       生成的总网关配置
  instances/sbx-<name>/
    compose.yml         生成的 compose（可 diff 可手工接管, D2）
    env.toml            该沙箱场景+版本选择
    Dockerfile.base     gen 产物（构建上下文用）
```

mgr 需要 **repo 根路径**（读 scenarios/ 目录 + Dockerfile.base.head/tail +
作为 docker build 上下文）：本地裸跑 = cwd/repo；容器形态 = 只读挂载 repo 根。
docker build 的构建上下文经 socket 发送，只读挂载即可。

### 3.3 SQLite schema（state.db）

```sql
CREATE TABLE sandboxes (
  name TEXT PRIMARY KEY,            -- 用户 slug，即子域名前缀
  created_at INTEGER NOT NULL,
  env_json TEXT NOT NULL,           -- {scenarios:[...], versions:{...}}
  env_hash TEXT NOT NULL,          -- sha256(canonical env + 片段内容)
  cpus REAL, mem_mb INTEGER,       -- deploy.resources.limits
  status TEXT NOT NULL,            -- running|stopped|creating|error|adopted
  adopted INTEGER DEFAULT 0,       -- 1 = 存量导入（compose 文件在外部）
  external_compose TEXT            -- adopted 时该栈 compose 文件路径
);
CREATE TABLE images (
  env_hash TEXT PRIMARY KEY,
  tag TEXT NOT NULL,               -- sandbox-base-<env_hash[:12]>
  built_at INTEGER,
  build_log TEXT                   -- 最后一次构建输出（失败原因展示）
);
CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT);
-- kv: models_config（canonical JSON，即上收的模型配置真相源）
```

- env_hash 计算：`sha256(排序后的 scenarios + versions + 每个启用片段文件
  内容 + head/tail 内容)`——片段被编辑后 hash 变化，触发重建（正确性优先）。
- 引用计数不单独建表：`SELECT count(*) FROM sandboxes WHERE env_hash=?`。

### 3.4 mgr API 面（mgr-web 消费）

```
GET    /api/sandboxes                 列表（含实时状态：compose ps 合并）
POST   /api/sandboxes                 创建（name/env/cpus/mem）→ 返回 job
GET    /api/sandboxes/:name           详情
PUT    /api/sandboxes/:name           改配置（env/资源；改 env 走重建流程）
DELETE /api/sandboxes/:name?volumes=1 删除（volumes=1 时 down -v，UI 确认）
POST   /api/sandboxes/:name/start|stop|restart
GET    /api/scenarios                 场景目录（config crate 发现逻辑）
GET    /api/images                    env-hash 镜像 + 引用数 + 构建状态
GET    /api/jobs/:id                  长任务轮询（创建/构建进度）
GET/PUT /api/models/config            canonical 模型配置（aio-models schema,
                                      对齐现有 app 契约: masked-echo merge）
POST   /api/models/discover|test      迁自 app 的同功能
GET    /api/usage                     轮询各沙箱 /api/models/usage 汇总
GET    /api/sandboxes/:name/entry_url 计算入口 URL（mgr-web 跳转用）
```

- 长任务（创建/构建/删除）走 job 队列（tokio task + 内存 + SQLite 状态），
  mgr-web 轮询 `/api/jobs/:id`；不做 SSE（个人规模轮询足够）。
- docker/compose 调用统一在 `mgr/src/docker.rs`：`tokio::process::Command`,
  `--format json` 输出解析（`docker compose -p X ps --format json`、
  `docker images --format json`），禁止文本 grep。

### 3.5 沙箱 compose 生成（mgr/src/composegen.rs）

以现 docker-compose.yml 为模板派生（mgr 内嵌模板常量 + 运行时插值）：

- project name：`sbx-<name>`（compose 文件内不写 name，由 `-p` 传入）。
- **全部 image: 引用，无 build: 段**——镜像由 mgr 显式 `docker build` 预构建：
  - `sandbox-base-<hash>`：`docker build -f instances/sbx-X/Dockerfile.base`
  - `sandbox-app-<hash>`：`docker build --build-arg BASE_IMAGE=sandbox-base-<hash>
    -f app/Dockerfile`（复用现有 ARG）
  - `sandbox-code-server-<hash>`：同上（code-server/Dockerfile 有 ARG BASE_IMAGE）
  - vnc 不依赖 base（FROM debian:bookworm-slim）→ 共享单 tag `sandbox-vnc`
  - caddy:2 官方镜像
- gateway：无 basicauth（Caddyfile 模板去掉 basicauth 块，无 secrets 挂载），
  无 ports 发布；加入 aio-mgr-net（alias `sbx-<name>`）+ 自有网络。
- app：无 ports 发布；environment 设
  `PI_WEB_URL` / `PI_WEB_ALLOWED_HOSTS`（见 §2.1）；加入 aio-mgr-net
  （alias `sbx-<name>-piweb`）；`deploy.resources.limits` 填 cpus/mem（D7）。
- code-server / vnc / base profile：结构与现状一致（侧车 netns、shm、tmpfs）；
  code-server/vnc 镜像 tag 换 env-hash 版。
- 卷：每 project 自带 `workspace` 卷（compose project 隔离天然成立）。

### 3.6 创建流程（R2）

```
POST /api/sandboxes
  1. 校验 name（slug：[a-z0-9-]+，不得撞已有）
  2. env 规范化 + 计算 env_hash
  3. 镜像检查：SELECT images WHERE env_hash → 未命中则入队构建
     （base → app → code-server → vnc 顺序；vnc 全局共享）
  4. gen：aio-config gen 逻辑（lib 调用）写 instances/sbx-X/Dockerfile.base
  5. composegen 写 instances/sbx-X/compose.yml
  6. docker compose -p sbx-X up -d
  7. 总网关 Caddyfile 重新生成 + reload
  8. 状态 → running；任一步失败 → 状态 error + build_log 保留
```

改 env（PUT）= 新 env_hash → 走同流程换镜像并 `up -d --force-recreate`
（卷保留，数据不丢）；旧镜像引用归零不自动删（镜像列表页手动清理）。

### 3.7 模型配置上收与同步（D6）

mgr 侧：`/api/models/*` 契约与现有 app 完全一致（masked-echo merge、
discover、test）——mgr-web 的模型页直接**移植 `web/src/panes/models/` 代码**
（fetch 路径相同，移植成本低）。真相源存 kv.models_config。

沙箱 app 侧改动：

- 新增 env `MGR_URL`（mgr 生成的 compose 中设
  `MGR_URL=http://mgr-api:8089`——mgr-api 也挂 aio-mgr-net，alias `mgr-api`）。
- 新后台任务：启动时 + 每 60s `GET {MGR_URL}/api/models/config`（带
  `?since=<版本号>` 的简化 ETag），响应与本地 canonical 不同则写盘 + 走现有
  render 派生链路；拉不到（mgr down/离线）→ 静默用本地缓存。
- `/api/models` 的 PUT/POST/DELETE 处理器在 `MGR_URL` 设置时返回
  `403 managed-by-mgr`；前端模型页检测到该错误切只读视图。
- usage：mgr 定时（60s）逐沙箱 `GET http://sbx-<name>:8088/api/models/usage`
  （经 aio-mgr-net 直达 app，沙箱 gateway 不参与），汇总入 `/api/usage`。

### 3.8 存量栈纳管（R7）

- mgr-web「导入现有栈」：输入现有 compose 文件路径（默认 repo 根
  docker-compose.yml），mgr：
  1. `docker compose -f <path> ps` 确认运行中；
  2. 登记 sandboxes（adopted=1, external_compose=path, env 从该栈现状推断
     或手工填，不强制）；
  3. `docker network connect --alias sbx-<name> aio-mgr-net <gateway容器>`、
     `--alias sbx-<name>-piweb aio-mgr-net <app容器>`；
  4. 生成总网关站点 + reload。
- adopted 沙箱支持：状态展示、启停（`docker compose -f path -p <project>`）；
  不支持改 env/资源（UI 明示「外部栈」）；删除 = 仅移除登记与路由，不删容器。
- 存量栈自身改造（一次性，随本任务落地）：gateway/Caddyfile 去 basicauth
  （D9，同步删 hash 机制与 Makefile hash target）、entrypoint 的
  PI_WEB_ALLOWED_HOSTS 可覆盖化（§2.1，对存量栈无行为影响，默认值不变）。

## 4. mgr-web（React SPA）

- `mgr-web/`：Vite + React + TS，复用 `web/` 的 styles.css token 体系
  （Kumo 视觉）与 i18n 模式；**不引 golden-layout**（纯管理页面）。
- 页面：
  1. **沙箱列表**：卡片（名称/状态/资源/镜像 tag/入口按钮「进入沙箱」新标签
     打开 `http://sbx-<name>.mgr.localhost/`）；操作：启停/重启/删除/编辑配置。
  2. **创建向导**：名称 → 场景勾选（always_on 场景锁定不可取消，语义对齐
     TUI）+ 版本下拉（GET /api/scenarios 数据驱动）→ CPU/内存 → 提交 →
     job 进度页（构建日志尾部实时展示）。
  3. **环境配置编辑**：同创建向导的场景/版本区，提交走 PUT（重建流程）。
  4. **模型配置页**：自 `web/src/panes/models/` 移植（供应商/agent/用量三块；
     用量页改为多沙箱汇总视图）。
  5. **镜像列表**：env-hash tag、构建时间、引用沙箱数、构建日志、手动删除。
- mgr-api 静态服务 mgr-web/dist（同 app 服务 web/ 的模式）。

## 5. 兼容与迁移

- 现有 make 流程（config/gen/build/up/down/save/load）**全部保持可用**
  （D4 兜底、D11 现状）；workspace 化后需回归 `make build` 与 CI。
- 存量单沙箱栈去 basicauth 是破坏性变更：README 更新（安全边界声明：
  信任边界在宿主机/本机）。
- app 镜像 ARG BASE_IMAGE 机制已存在，无新兼容面。
- 回滚：mgr 是纯新增栈（+ 存量栈 Caddyfile 一处删改），回滚 = 停 mgr 栈 +
  还原 gateway/Caddyfile 单 commit；沙箱 compose 文件均在 instances/ 可手工
  `docker compose -f ... down`。

## 6. 主要权衡记录

- **compose CLI vs Docker API**（D2 已定）：生成物可接管 > 类型安全。
- **共享 aio-mgr-net vs 宿主端口段**：选共享网络——沙箱零宿主端口暴露，
  路由集中；代价是 adopted 存量栈需要手工 network connect。
- **mgr 生成 Caddyfile + reload vs 动态 API**：文件生成可 diff、可审查；
  reload 有秒级生效延迟，个人规模无碍。
- **模型同步拉取 vs 推送**：沙箱 down 时推送丢失，拉取自愈；代价是配置
  生效有最长 60s 延迟。
- **抽 aio-models crate vs 复制 schema**：单一 owner 原则（cross-layer guide）；
  代价是一次 workspace 重构。app 的 models 路由文件改动集中在 import 路径。

## 7. 风险与对策

- **workspace 化碰 app 构建**：app/Dockerfile 的 COPY/Cargo.lock 路径变化
  可能使 dep-cache 层失效——首次构建变慢，非功能风险；验证 `make build`。
- **compose CLI JSON 输出版本差异**：锁定解析失败时报错而非静默空列表。
- **caddy reload 失败**：保留上一份 Caddyfile.bak；状态页展示 reload 错误。
- **pi-web Host 白名单**：双保险（网关重写 Host + PI_WEB_ALLOWED_HOSTS），
  A4 验收覆盖。
