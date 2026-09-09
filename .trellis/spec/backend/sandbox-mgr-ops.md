# Sandbox-mgr 控制平面运维契约

> **Purpose**: sandbox-mgr(aio-mgr)在 DooD 形态下驱动宿主 docker 的硬约束。
> 来源任务: 09-08-sandbox-mgr-tui Phase 2(2026-09-08)。这些是实测踩坑沉淀
> 的可执行契约,不是建议——违反任何一条都会以"看起来成功"的方式失败
> (reload 报 ok、compose up 正常,但路由不通/挂载为空)。
> 代码锚点: `mgr/src/{caddy.rs,docker.rs,composegen.rs}`、`mgr/compose.yml`、
> `mgr/Dockerfile`。

---

## 契约 1: DooD 路径同一性(mgr-api 容器挂载)

**Trigger**: 任何在容器内通过 docker.sock 跑 `docker compose`/`docker build`
的控制面(当前仅 mgr-api)。

mgr-api 生成的沙箱 compose 使用**相对 bind 挂载**(`./gateway/Caddyfile`)。
compose CLI 在**自己的文件系统**上把相对路径解析为绝对路径再交给 daemon,
daemon 在**宿主文件系统**上解析这个绝对路径。两个路径空间若不一致,
bind 源会被 daemon 当作不存在的主机路径——docker 静默创建一个空目录挂进去,
`compose up` 成功、容器 Running、但配置文件为空。

**规则**: mgr 栈 compose 里 repo 与 mgr-data 必须挂载在**与宿主绝对路径完全
相同的容器路径**上:

```yaml
# mgr/compose.yml (正确)
environment:
  MGR_REPO: ${PWD:?}
  MGR_DATA: ${PWD:?}/mgr-data
volumes:
  - ${PWD:?}:${PWD:?}:ro            # 同一绝对路径,不是 /repo:ro
  - ${PWD:?}/mgr-data:${PWD:?}/mgr-data
```

并且 Makefile 必须用 `MGR_COMPOSE := PWD=$(CURDIR) $(COMPOSE) ...` 钉住
PWD——`make -C <repo> mgr-up` 时继承的 PWD 是调用者 cwd,会静默挂错目录。

**验证点**: 创建沙箱后 `docker inspect <app容器> --format '{{json .Mounts}}'`,
bind 源必须是宿主绝对路径且内容非空。

---

## 契约 2: 被 bind mount 的生成文件禁止 rename 更新

**Trigger**: mgr-api 生成 mgr-data/caddy/Caddyfile(总网关配置)。

mgr-gateway 只读挂载该文件。`rename + 新建`的更新方式会换掉 inode——
bind mount 跟踪的是**旧 inode**,网关(以及 `docker exec` 进去执行的
`caddy reload`)永远读到旧内容,且 reload 日志报 `config is unchanged`
(最具迷惑性的失败:一切显示成功)。

**规则**: 备份用 `fs::copy`(拷内容,inode 无关),更新用 `fs::write`
(O_TRUNC 原地截断写,inode 不变):

```rust
// mgr/src/caddy.rs (正确)
if path.exists() {
    let _ = std::fs::copy(&path, path.with_extension("bak"));  // 备份=拷贝
}
std::fs::write(&path, render(&rows))?;                          // 原地写

// 错误: rename 换 inode,bind mount 从此指向 .bak 的旧 inode
```

**验证点**: 改动 Caddyfile 后 `docker logs aio-mgr-gateway-1` 不应出现
`config is unchanged`(除非内容真的没变)。

---

## 契约 3: 总网关 Host 重写必须用公共子域名

**Trigger**: 任何反向代理到 pi-web(`sbx-<name>-piweb:30141`)的网关配置。

pi-web 的 request-security 中间件(middleware.js)对 Host 的实际匹配规则:

1. 剥端口(`new URL('http://'+host).hostname`),再校验 hostname;
2. 放行: `localhost` / **`*.localhost` 后缀** / IP 字面量 /
   `PI_WEB_ALLOWED_HOSTS` 条目(逗号分隔,同样剥端口比较);
3. 其余一律 403 `Untrusted request`(页面路径)或 403
   `Untrusted API request`(/api)。

**规则**: 总网关重写 Host 时写**公共子域名**,不是上游 alias:

```caddyfile
# 正确 (mgr/src/caddy.rs render): *.localhost 后缀命中规则 2
header_up Host sbx-<name>-piweb.mgr.localhost

# 错误: 裸 alias 不以 .localhost 结尾、不在 ALLOWED_HOSTS → 403
# header_up Host sbx-<name>-piweb:30141
```

注意与 `PI_WEB_ALLOWED_HOSTS`(compose 里 `app,sbx-<name>-piweb.mgr.localhost`)
是双保险关系:即使 env 丢失,`*.localhost` 后缀仍放行。design.md §2 早期版本
写的是 alias 形式,已于 09-08 更正——以本文为准。

---

## 契约 4: mgr 生命周期命令必须带全量 profile

mgr 沙箱的 code-server/vnc 在生成的 compose 里是 profile 门控服务(与 repo
compose 同构,design §3.5),但 mgr 把沙箱当整体产品管理:镜像全部构建、
up 必须全起(否则工作台面板缺一半)、down 必须看到它们才能拆干净。

```rust
// mgr/src/docker.rs
const SANDBOX_PROFILES: [&str; 4] = ["--profile", "code-server", "--profile", "vnc"];
```

`compose_ps` 例外: compose 5.x 的 `ps --all` 列出 profile 门控容器,
不需要(也不应该)加 profile 标志。

**验证点**: mgr 创建的沙箱应有 4 个容器(app/gateway/code-server/vnc);
`GET /api/manifest` 中 codeServer/vnc/piWeb 全部 `enabled: true`。

---

## 契约 5: mgr/Dockerfile 构建须知

- 运行时 CLI 从 `docker:cli` 镜像 COPY(bookworm 的 docker.io 太老且无
  compose/buildx 插件,mgr 第一个 `docker compose ps` 就会死),插件路径
  `/usr/local/libexec/docker/cli-plugins/`。
- mgr 依赖 aio-config **lib**(scenario/gen),Phase 4 起也依赖 aio-models
  **lib**(canonical schema/mask/merge/validate 单一 owner)——dep-cache 层
  与真实源层都必须拷 aio-models/src,touch 清单含其 `lib.rs`。
- 依赖路径 crate 的 lib 时,真实源层的 touch 清单必须包含其 `lib.rs`
  (BuildKit COPY-mtime 陷阱见
  [CI Image Conventions 约定 7](../guides/ci-image-conventions.md));
  dep-cache 层必须为**每个** workspace member 写 dummy 源,漏一个
  `cargo build -p` 直接报 target resolution error。
- **web-builder 阶段(Phase 3)**: mgr 无 sandbox-base 依赖,用
  `node:20-bookworm-slim AS web-builder`(`COPY mgr-web/package*.json` →
  `npm ci` → `COPY mgr-web` → `npm run build`),runtime 阶段 `COPY
  --from=web-builder /mgr-web/dist /app/static` 并 `ENV MGR_WEB_DIR=/app/static`。
  npm 项目不是 cargo member,dep-cache 虚拟源层不受影响。

---

## 契约 6: mgr.localhost 静态站点 + mgr-web 静态服务(Phase 3)

**Trigger**: mgr-web 自身的访问入口;任何修改 caddy.rs render() 的人。

mgr-api 在 compose 里**不发布宿主端口**——mgr-web 只经总网关
`http://mgr.localhost` 访问。render() 生成的 Caddyfile **第一个站点块固定为**:

```caddyfile
http://mgr.localhost {
    reverse_proxy mgr-api:8089
}
```

上游 `mgr-api` 名来自 mgr-net 上的服务名(compose 双网: mgr-net +
aio-mgr-net,alias `mgr-api` 保留给 Phase 4 沙箱侧 MGR_URL 拉取)。

**静态服务**: mgr-api 用 tower-http `ServeDir::new(dir).fallback(
ServeFile::new(dir/index.html))` 服务 SPA(同 app 模式);dist 路径 env
`MGR_WEB_DIR` 覆盖,默认 `<repo>/mgr-web/dist`(裸跑形态);容器形态
Dockerfile 烘 `/app/static`(见契约 5)。`/api` seam 三路由(app 同构)
注册在 ServeDir fallback 之前。

**验证点**: `curl -s -H 'Host: mgr.localhost' http://localhost/` 返回
index.html;`/api/sandboxes` 返回 JSON 而非 HTML;未知 `/api/*` 404 JSON。
render() 单测 `render_has_static_mgr_site_first` 锚定站点块存在且在最前。

---

## 契约 7: 模型配置上收——同步链与写降级(Phase 4)

**Trigger**: 任何动 `mgr/src/models.rs`、`app/src/mgr_sync.rs`、
`app/src/routes/models/mod.rs` 写接口、或 composegen MGR_URL 注入的人。

mgr 是模型配置唯一真相源(D6),三段式同步链,任何一段的字段名/语义
漂移都会让拉取静默失效(拉不到≠报错,是 60s 空转):

1. **真相源**: kv 表 `models_config` 键,值 `{"version": <u64>, "config":
   <CanonicalConfig>}`,PUT 成功才 bump version。kv 只可能写入通过
   validate 的 JSON,因此 mgr 侧无 app 的 corrupt-move-aside 分支
   (app models.json 是文件、mgr 是 kv——损坏语义不同是**有意的**)。
2. **拉取端点**: `GET /api/models/sync` 返回**未 mask** canonical(明文
   key)。这是 D6/D9 已接受的边界: mgr-api 不发布宿主端口、aio-mgr-net
   不出宿主、总网关站点块只按 Host 路由沙箱域名。**不要**在 mgr-web 里
   调它(要明文没意义),浏览器走 masked 的 `GET /api/models/config`。
   消费端 `app/src/mgr_sync.rs` 的解析结构体与 mgr 的响应形状有处理器
   级测试双向锁定(`sync_handler_shape_*` / `sync_payload_decodes_*`)。
3. **沙箱侧**: composegen 给 app 注入 `MGR_URL=http://mgr-api:8089`;
   启动拉一次 + 60s 周期;深比较(serde_json 全量等值)不同才
   `write_config` + `apply_all_agents`(与 apply/:agent handler 共用
   `render_agent`,单一渲染路径);拉取失败 warn 一次静默用本地缓存。
   `MGR_URL` 未设置 = 存量栈,零行为变化(guard 恒通、不 spawn)。

**写降级矩阵**: MGR_URL 设置时 app 侧 6 个写接口统一 403 body
`managed-by-mgr`(PUT config、import/pi、apply/:agent、provider PUT/
DELETE、sync);前端 `GET /api/models/managed` 探测只读态 + 403 兜底。
GET 类(config/agents/usage/catalog/managed)与 discover/test 探测**不降级**。

**usage 扇出**: `GET /api/usage?window=` 按需并发拉各 running 沙箱
`http://sbx-<name>-piweb:8088/api/models/usage`(注意别名是
**sbx-<name>-piweb**——composegen 给 app 的 aio-mgr-net 别名;design §3.7
原文的 `sbx-<name>:8088` 是笔误,`sbx-<name>` 是 gateway 的 :80 别名)。
单沙箱 5s 超时、错误隔离进 `error` 字段、30s TTL 缓存(含错误条目,
避免错误沙箱被高频重试)。design 原文的"60s 常驻轮询"实现为按需扇出
+TTL——等价满足汇总语义,少一份常驻状态。

**验证点**: 改 mgr 配置后沙箱 canonical(明文)与 pi native render 在
≤60s 内更新;沙箱 PUT 返回 403 `managed-by-mgr`;`/api/usage` 含各
running 沙箱条目且单沙箱挂掉不整体失败。

**mgr 不提供的端点**(沙箱本地文件操作,mgr 语义不成立,mgr-web 移植
时裁掉): `/api/models/agents`、`apply/:agent`、`agents/:agent/provider/
:id`、`agents/:agent/sync`、单沙箱 `/api/models/usage`。
