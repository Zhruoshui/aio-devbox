# Sandbox-mgr 控制平面运维契约

> **Purpose**: sandbox-mgr(aio-mgr)在 DooD 形态下驱动宿主 docker 的硬约束。
> 来源任务: 09-08-sandbox-mgr-tui Phase 2(2026-09-08)+ 09-09-sandbox-mgr-
> unified(契约 4 改写/契约 7 多 profile/契约 9 第四处/契约 10 新增,2026-09-10)
> + 09-10-mgr-subdomain-port-follow(契约 3 改写: Host 透传禁改写,2026-09-10)。
> 这些是实测踩坑沉淀的可执行契约,不是建议——违反任何一条都会以"看起来
> 成功"的方式失败(reload 报 ok、compose up 正常,但路由不通/挂载为空)。
> 代码锚点: `mgr/src/{caddy.rs,docker.rs,composegen.rs,proxy.rs,models.rs}`、
> `mgr/compose.yml`、`mgr/Dockerfile`。

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

## 契约 3: 总网关到 pi-web 禁止改写 Host——透传浏览器原始 Host

**Trigger**: 任何反向代理到 pi-web(`sbx-<name>-piweb:30141`)的网关配置。

pi-web 的 request-security 中间件(middleware.js)对 Host 的实际匹配规则:

1. 剥端口(`new URL('http://'+host).hostname`),再校验 hostname;
2. 放行: `localhost` / **`*.localhost` 后缀** / IP 字面量 /
   `PI_WEB_ALLOWED_HOSTS` 条目(逗号分隔,同样剥端口比较);
3. 其余一律 403 `Untrusted request`(页面路径)或 403
   `Untrusted API request`(/api)。

**规则**: 总网关**不改写 Host**(无 `header_up Host`),让浏览器的原始
Host(含宿主发布端口,如 `sbx-<name>-piweb.mgr.localhost:8081`)原样透传:

```caddyfile
# 正确 (mgr/src/caddy.rs render, 09-10 定稿):
http://sbx-<name>-piweb.mgr.localhost {
    reverse_proxy http://sbx-<name>-piweb:30141
}

# 错误 1: 重写成无端口子域字面量——pi-web 对 /api/* 要求
# Origin == 由 Host 推导的 origin;宿主非 80 端口(如 8081)发布时浏览器
# Origin 带 :8081 而改写后的 Host 不带 → 403 "Untrusted API request"
# (09-10 实测, mgr-subdomain-port-follow R7)。
# header_up Host sbx-<name>-piweb.mgr.localhost
# 错误 2: 裸 alias 不以 .localhost 结尾、不在 ALLOWED_HOSTS → 403。
# header_up Host sbx-<name>-piweb:30141
```

浏览器经总网关访问时 Host 恒为公共子域名(可带端口),规则 1 的剥端口 +
规则 2 的 `*.localhost` 后缀天然放行;`PI_WEB_ALLOWED_HOSTS`(compose 里
`app,sbx-<name>-piweb.mgr.localhost`)是沙箱网内部直连 `http://app:30141`
的第二保险。历史上曾要求重写为公共子域名(09-08)——那是为了避开裸 alias
403;在宿主端口跟随上线后,改写本身成了 403 根源,故改为透传(09-10)。
单测锚定: `!out.contains("header_up Host")`(caddy.rs render)。

---

## 契约 4: mgr 生命周期命令的 profile 分裂——up 只带 vnc(按沙箱可选),其余全量

mgr 沙箱的 code-server/vnc 在生成的 compose 里是 profile 门控服务(与 repo
compose 同构,design §3.5)。unified Phase 3(D4,code-server 按需实例)把
原"全量 profile"契约分裂为两半;S1(09-10-mgr-create-services)再把 vnc
从"up 必带"改成**按沙箱是否安装可选用**:

```rust
// mgr/src/docker.rs
/// up 专用: 仅 vnc,且按沙箱服务开关条件化(装了才带)
fn up_profiles(include_vnc: bool) -> Vec<&'static str> {
    if include_vnc { vec!["--profile", "vnc"] } else { Vec::new() }
}
/// 非 up 生命周期(stop/restart/down/rm): 全量
const SANDBOX_PROFILES: [&str; 4] = ["--profile", "code-server", "--profile", "vnc"];
/// 单服务按需拉起(code-server): 仅其自身 profile
pub const CODE_SERVER_PROFILE: [&str; 2] = ["--profile", "code-server"];
```

`compose_up` 增 `with_vnc: bool` 参:create/restart 传 `services.vnc`,
start handler 同;adopt 走 `compose_up_file` 变体**保持无条件 vnc 标志**
(外部 compose 未知,无 vnc 服务的 compose 不受未匹配 profile 影响)。

**为什么 up 不带 code-server**: vnc 是常驻依赖(pi agent-browser 硬依赖
其中的 CDP Chromium),code-server 是纯编辑面、无任何东西依赖其常驻。
工作区 code-server pane 打开时经 `POST /api/sandboxes/:name/service/
code-server/start`(`compose_service_up`)按需拉起。实测(compose 5.2.0)
不带 code-server profile 的 `up` 不会启动它、也**不触碰**已启动的实例。

**S1 的 vnc 条件化**: 无 vnc 服务的沙箱 compose 压根没有 vnc 段,带
`--profile vnc` 静默匹配不到任何服务——harmless 但产生噪音日志,故
S1 改为不传。**up 后码容器数随开关变**:全开沙箱仍是 3 容器
(app/gateway/vnc),无 vnc 沙箱是 2(app/gateway)。

**为什么其余命令必须全量**: stop/restart/down 必须能**看见** code-server
才能停它/拆干净(已拉起的 code-server 随沙箱一起死,绝不残留)。实测
这些命令对"服务无容器"也容忍(fresh D4 沙箱直接 stop/down 不报错)。

**recreate 的 zombie 陷阱**(jobs.rs): app 容器被 force-recreate 后,旧的
code-server 容器仍 running 但挂在**已删除**旧 app 的 netns
(`network_mode: service:app`)——死网络,下次全量 restart 报错。recreate
job 必须先 `compose_service_rm`(targeted `rm --force --stop`)再 up。
`up -d <svc>` 会按当前 app 容器重建服务。

`compose_ps` 例外: compose 5.x 的 `ps --all` 列出 profile 门控容器,
不需要(也不应该)加 profile 标志。

**验证点**: 新建沙箱 up 后 3 容器 running(app/gateway/vnc),code-server
无容器;打开 pane 后 4 容器;stop 后全停;`GET /api/manifest` 中
codeServer/vnc/piWeb 全部 `enabled: true`(门控的是容器,不是 manifest)。

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

## 契约 7: 模型配置上收——多 profile 同步链与写降级(Phase 4 → unified Phase 4)

**Trigger**: 任何动 `mgr/src/models.rs`、`app/src/mgr_sync.rs`、
`app/src/routes/models/mod.rs` 写接口、或 composegen MGR_URL/MGR_SANDBOX_NAME
注入的人。

mgr 是模型配置唯一真相源(D6),**per-profile**(unified Phase 4, D8):
一套 profile = 一整份 CanonicalConfig(aio-models 不感知包装层),每个
沙箱指派一套。三段式同步链,任何一段的字段名/语义漂移都会让拉取静默
失效(拉不到≠报错,是 60s 空转):

1. **真相源**: kv 表 `models_profiles` 键,值 `{"version": <u64 全局递增>,
   "profiles": [{"id", "name", "config": <CanonicalConfig>}], "assignments":
   {"<sandbox-name>": {"profile": "<id>" | null, "agents": <subset> | null}}}`。
   PUT 成功才 bump version(version 全局非 per-profile——拉取端深比较,
   跨 profile 的 bump 只多一次跳过比较)。kv 只可能写入通过 validate 的
   JSON,因此 mgr 侧无 app 的 corrupt-move-aside 分支(app models.json 是
   文件、mgr 是 kv——损坏语义不同是**有意的**)。
   **S2 assignments 形状**: value 从裸字符串升级为 `{profile, agents}`
   (design §1.1)。反序列化兼容旧裸字符串 `"<profile-id>"`(= `agents:
   None` 全指派,AC4);`agents` 语义: None = 全指派、`[]` = 零指派、
   `[names]` = 精确子集。旧代码读新形状会失败——**回滚前须手工把 kv
   assignments value 改回裸字符串**(implement 回滚节)。
2. **迁移(双向兼容)**: 读到旧键 `models_config`(单配置时代)→ 转为
   `id: "default"` profile + **全部现存沙箱指派到它**(升级行为零变化),
   先写新键再删旧键——中间崩溃则下次 boot 幂等重跑;回滚的 mgr 读到
   新键缺失 → 读回旧键。`default` id 与 `gen_preset_id` 风格的
   `profile-<5hex>` id 均由后端持有。
3. **拉取端点**: `GET /api/models/sync?name=<sandbox>` 返回该沙箱**所指派
   profile** 的**未 mask** canonical(明文 key)+ `agents` 子集字段
   (S2: None/缺省 = 全指派;数组 = 精确子集)。**404 矩阵**(app 按
   "未指派,保持本地"静默处理): 无 name / 未知沙箱 / 未指派 / **零指派
   (agents: [])**,四种 404 且响应**不可区分**(不向任意调用方泄露沙箱名
   存在性;零指派让沙箱保持本地 = AC3)。明文边界同 D6/D9 已接受:
   mgr-api 不发布宿主端口、aio-mgr-net 不出宿主。**不要**在 mgr-web 里调
   它,浏览器走 masked 的 `GET /api/models/config?profile=`。请求与响应
   形状有处理器级测试双向锁定(`sync_handler_shape_*`(现含 agents) /
   `sync_payload_decodes_*` + `sync_url_carries_sandbox_name_query`,新增
   `sync_payload_carries_agents_and_empty_agents_404s`)。
4. **沙箱侧**: composegen 给 app 注入 `MGR_URL=http://mgr-api:8089` +
   `MGR_SANDBOX_NAME=<name>`(拉取身份);启动拉一次 + 60s 周期;深比较
   (serde_json 全量等值)**config + 上次应用的 agent 子集**(S2——子集变了
   也要重渲染,`last_agents` 存于 loop 状态,进程重启后首拉重渲,幂等)
  不同才 `write_config` + `apply_selected_agents`(过滤变体;None = 全量
   = 旧 `apply_all_agents` 语义,与 apply/:agent handler 共用
   `render_agent`,单一渲染路径,四个 renderer 零改动)。失败语义两档:
   **404 = debug + 保持本地**(未指派/解绑/零指派契约,不是错误);
   其余(transport/非 200/解析失败)= warn 一次 + 保持本地。两者都不写
   不退。`MGR_URL` 未设置 = 存量栈,零行为变化(guard 恒通、不 spawn);
   有 MGR_URL 无 MGR_SANDBOX_NAME(旧 compose)= 发无名请求,mgr 404,
   同样静默保持本地。
4. **沙箱侧**: composegen 给 app 注入 `MGR_URL=http://mgr-api:8089` +
   `MGR_SANDBOX_NAME=<name>`(拉取身份);启动拉一次 + 60s 周期;深比较
   (serde_json 全量等值)不同才 `write_config` + `apply_all_agents`(与
   apply/:agent handler 共用 `render_agent`,单一渲染路径)。失败语义
   两档: **404 = debug + 保持本地**(未指派/解绑契约,不是错误);
   其余(transport/非 200/解析失败)= warn 一次 + 保持本地。两者都不写
   不退。`MGR_URL` 未设置 = 存量栈,零行为变化(guard 恒通、不 spawn);
   有 MGR_URL 无 MGR_SANDBOX_NAME(旧 compose)= 发无名请求,mgr 404,
   同样静默保持本地。

**指派端点**: `PUT /api/sandboxes/:name/model_profile`,body
`{"profile": "<id>" | null, "agents": <subset> | null}`(profile null/缺失
= 解绑)。**agents 整份替换**: 省略/缺省 = 全指派(旧客户端兼容,AC4)、
`[]` = 零指派(沙箱下一拉 404 → 保持本地)、数组 = 精确子集(仅渲染
勾选 agent,未勾选 agent 的本地配置不动,R3)。未知 agent 名 400
(`VALID_AGENTS` 白名单)。纯 kv 写,**绝不触发 recreate**——沙箱下轮 60s
拉取生效;与 `PUT /api/sandboxes/:name`(env 改动走 recreate job)是两条
独立路由,不得合并。adopted 行同样可指派(无 MGR_URL 不拉取,指派惰性
记录)。**删除/注销沙箱必须同步清掉其 assignment**(jobs.rs delete 与
unadopt 都带)——残留条目会被同名新建的沙箱静默继承。`sandbox_json`
增 `model_profile`(id 或 null)+ `model_agents`(null = 全指派,旧数据
兼容)。profile CRUD: 删最后一个 profile 400;删除时解绑其全部 assignments。

**写降级矩阵**(不变): MGR_URL 设置时 app 侧 6 个写接口统一 403 body
`managed-by-mgr`(PUT config、import/pi、apply/:agent、provider PUT/
DELETE、sync);前端 `GET /api/models/managed` 探测只读态 + 403 兜底。
GET 类(config/agents/usage/catalog/managed)与 discover/test 探测**不降级**。

**usage 扇出**: `GET /api/usage?window=` 按需并发拉各 running 沙箱
`http://sbx-<name>-piweb:8088/api/models/usage`(注意别名是
**sbx-<name>-piweb**——composegen 给 app 的 aio-mgr-net 别名;design §3.7
原文的 `sbx-<name>:8088` 是笔误,`sbx-<name>` 是 gateway 的 :80 别名)。
单沙箱 5s 超时、错误隔离进 `error` 字段、30s TTL 缓存(含错误条目,
避免错误沙箱被高频重试)。

**验证点**: 建两个 profile 指派不同沙箱,改 A 所指 profile 后 A 的
canonical(明文)与 native render 在 ≤60s 内更新、B 不动;解绑后该沙箱
保持本地(404 → debug 不写);旧 `models_config` 键升级后自动迁移且全
沙箱行为不变;`/api/usage` 含各 running 沙箱条目且单沙箱挂掉不整体失败。
**S2 (AC2-AC4)**: 沙箱 A 指派 profile P + 仅 pi/opencode → ≤60s 内
`~/.pi` 配置更新、`~/.claude`/`~/.codex` 不动;agent 全不勾(零指派)→
拉取后本地配置完全不动;旧 kv `{"<sbx>": "<profile-id>"}` 形状读取为
全指派不回归;`agentSubsetSummary` 的 `pi+2` 摘要与 EditPage/快捷指派
的勾选 → PUT → 60s 生效闭环。

**mgr 不提供的端点**(沙箱本地文件操作,mgr 语义不成立,mgr-web 移植
时裁掉): `/api/models/agents`、`apply/:agent`、`agents/:agent/provider/
:id`、`agents/:agent/sync`、单沙箱 `/api/models/usage`。

---

## 契约 8: 存量栈纳管(adopt)——别名三方一致 + 外部 compose 无 -p(Phase 5)

**Trigger**: 任何动 `mgr/src/routes.rs` adopt 流程、`mgr/src/docker.rs`
外部 compose 变体、或试图纳管非 mgr 生成栈的人。

沙箱身份 = aio-mgr-net 上的两个网络别名,由**三方**共同约定,任何一方
漂移都会死路由(总网关 200 空响应或 proxy unreachable):

| 别名 | 连接方 | 消费方 |
|------|--------|--------|
| `sbx-<name>`(gateway 容器) | adopt 时手工 connect;mgr 生成栈由 composegen aliases | caddy.rs render `reverse_proxy sbx-<name>:8080` |
| `sbx-<name>-piweb`(app 容器) | 同上 | render piweb 块 + usage 扇出 URL |

**外部 compose 生命周期规则**:

1. **不带 `-p`**: 外部栈的 project 名由 compose 从文件所在目录推导
   (存量栈 `make up` = 目录名如 `aio`)。显式传 sbx- 前缀 project 会
   打错目标/凭空起第二套容器。`compose_*_file` 变体因此与 mgr 栈
   变体并存,不可合并。profile 仍带全量(契约 4 同理)。
2. **start 后必须重连别名**: 外部栈的 compose 里没有 network connect,
   down/up 重建容器后别名**必然丢失**(D2 已知代价)。adopted start =
   `compose up -d` + fresh ps 拿容器名 + 两个 network connect。容器名
   不可持久存(recreate 会变),每次现查。
3. **adopt 校验**: 相对路径相对 `MGR_REPO` 解析;`compose -f <path> ps`
   无 running 条目 → 400(先 `make up`);gateway/app 服务名默认
   "gateway"/"app" 可覆盖,但**仅 adopt 时刻生效**(schema 无列,start
   重连用默认名——自定义服务名栈 stop→start 后显式报错而非静默死路由)。
4. **unadopt 只去登记**: 删行 → best-effort 断连两个容器(ps 失败仅
   warn,不阻断)→ caddy regenerate。**绝不 down 外部容器/卷**。
   caddy 写文件失败时行已删、接口 500、重试报 not found——注释已声明
   best-effort 自愈(同名 re-adopt 覆盖残留别名)。
5. **路由失效语义**: 注销后 `curl -H 'Host: sbx-x.mgr.localhost'` 返回
   **空 200**(caddy 无 catch-all 站点时未知 Host 的默认行为),不是
   404。判定"域名失效"用响应体大小(0 字节)或对比 mgr.localhost。

**验证点**: adopt 后列表出现 adopted 行(image=external);stop→start
后别名仍在(`docker inspect ... Aliases`);DELETE 后容器仍 running、
别名消失、Caddyfile 无该站点块、子域名 0 字节响应。

---

## 契约 9: 全栈无认证——安全边界与残留清理(Phase 5, D9)

**Trigger**: 任何想给网关/mgr 加回认证、或在不受信网络部署的人。

D9 决策: 信任边界 = 宿主机/本机。**全面无认证**——存量栈 gateway
(去 basicauth 后的 repo gateway/Caddyfile)、mgr 总网关(caddy.rs
render,有单测 `render_never_contains_basicauth` 锚定)、每沙箱生成的
gateway(composegen,同锚定)、**mgr-api 的 `/api/sbx/:name/*` 沙箱代理**
(unified Phase 1 第四处——它把每沙箱 app 的 pty(`/api/term/ws`,
全 shell 面)收拢到 mgr.localhost origin 下,等价于网关层的暴露面;
见契约 10)。

- 重新引入认证必须四处同步: repo Caddyfile + caddy.rs render +
  composegen render_caddyfile + proxy.rs(任一遗漏 = 部分路由裸奔)。
- 历史机制已删,不可"顺手恢复": `make hash`/`ensure-hash`、
  gateway/secrets/、gateway/entrypoint.sh(hash 投递)、Makefile
  save/load 的 hash 打包、CI 冒烟的 `-u admin:admin`。
- `docker-compose.yml` gateway 的 `env_file: .env` **保留**(PI_WEB_HOST_PORT
  等仍需要;SANDBOX_USER 残留在旧 .env 里是无害未引用 env)。
- 部署边界: mgr 总网关 `ports: 80:80` 是**宿主入口**,任何 LAN 暴露
  = 无认证暴露全部沙箱 + mgr 控制面。远程使用自行加 VPN/带认证反代。

**验证点**: `grep -rn "basicauth\|SANDBOX_USER\|ensure-hash\|secrets/hash"
Makefile docker-compose.yml gateway/ .env.example .github/ README* docs/`
清零;`make -n up NOBUILD=1` 无 ensure-hash 报错;网关直连 200 无
WWW-Authenticate 头。

---

## 契约 10: /api/sbx/:name/* 沙箱代理——宿主封闭派生(unified Phase 1)

**Trigger**: 任何动 `mgr/src/proxy.rs`、在 mgr-web 里直连沙箱后端、
或想加第五条代理面的人。

mgr-web 的一切沙箱面(终端 WS、buttons CRUD、manifest、/preview)经
mgr-api 代理而非浏览器跨子域直连——理由: adopted 旧栈 app 镜像**无 CORS
头**(直连必死)+ 未来认证单一收口(mgr.localhost 一个 origin)。

**规则**:

1. **上游宿主封闭派生**: 上游恒为 `http://sbx-<name>-piweb:8088/<path>`,
   `<name>` 取自路由参数,必须过 `validate_name` **且**存在于 sandboxes
   表(native/adopted 皆可——adopt 把同一别名接入 aio-mgr-net,契约 8)。
   **任何请求参数都到不了上游 HOST——无 SSRF 面**,只是注册沙箱的
   name-keyed 映射。别名是 app 的 aio-mgr-net 别名(同 usage 扇出的
   选择),不是 gateway 的 `sbx-<name>`。
2. **name 取原始(未解码)路径段**: slug 字母表 [a-z0-9-] 永不需要
   percent-decode,名字里的任何转义序列按定义不是注册沙箱,被
   validate_name 拒绝——上游宿主永不由解码值构造。
3. **HTTP + WS 双透传**: HTTP 剥 hop-by-hop 头 + `Body::from_stream`
   流式(SSE 存活);WS 以 app `/preview` 代理为模板(Upgrade 探测 /
   2s 上游握手预算 / subprotocol 回传 / 双向消息泵)。错误走 mgr
   `{"error": ...}` JSON 形状(mgr-web apiError 只解 JSON);未知
   `:name` = **真 404**(代理语义,非 lifecycle 的 400-shaped not found)。
4. **不做存活过滤**: stopped 沙箱连接失败 → 502(工作区树层面置灰,
   代理不二次猜测)。
5. **路由形状**: 只注册 `/api/sbx/:name/*path`(matchit 0.7.3 catch-all
   需非空尾段);裸 `/api/sbx/<name>` 回落到 `/api/*rest` seam 404;
   尾斜杠形式不匹配任何路由(unmatched_fallback)。代理 router 经
   merge 注册在 seam 之前。

**验证点**: `curl http://mgr.localhost/api/sbx/<name>/api/manifest` 返回
该沙箱 manifest;终端 pane 经代理打字/resize 可用;未知沙箱 404 JSON;
`curl http://mgr.localhost/api/sbx/<name>/preview/<port>/` 用户 web 按钮
经代理可达。

---

## 契约 11: 共享网络服务别名唯一性——localhost 探测 + sbx-\<name\>-app 反代(09-20)

**Trigger**: 任何给 `app/services.toml` 加/改 `target`、在 composegen 里
写反代上游、或在 mgr-web 侧按 manifest `enabled` 过滤按钮的人。

**事故形态**(2026-09-20 实测,任务 09-20-sandbox-service-buttons):
每个沙箱的 app 容器都加入**共享外部网络** `aio-mgr-net`,compose 服务
名 `app` 在该网络上**跨沙箱同名**——于是:

- `app/services.toml` 的 web 探测 `app:8200/6080/30141` 从本沙箱 app
  发起,DNS 却解析到**别的沙箱**的 app → 未安装的服务误报
  `enabled: true`(最小沙箱 min1 的 codeServer/vnc/piWeb 全误报,来源
  是 dev1)。
- mgr 生成 Caddyfile 的 `reverse_proxy app:<port>` 同样歧义 → min1 的
  `/code-server/`、`/vnc/` 返回 **200,实际代理 dev1 的编辑器/桌面**
  (跨沙箱数据泄漏,读写双路径)。

**规则**(修复后形态,勿回退):

1. **services.toml 的 `target` 必须 localhost**(sidecar 共享 app netns,
   pi-web 容器内自起):`localhost:8200` / `localhost:6080` /
   `localhost:30141`。localhost 探测只反映本沙箱。
2. **composegen 反代上游必须唯一别名**:app 在 sandbox-net 上带
   `sbx-<name>-app` 别名,生成 Caddyfile 所有 `reverse_proxy`(含
   catch-all `:8088`)拨 `sbx-<name>-app:<port>`。sandbox-net 是
   project-scoped,别名天然唯一。
3. **未安装即无路由**:Caddyfile 的 `/code-server/*`、`/vnc/*` 块按
   `db::Services` 裁剪——未安装的服务不生成路由(落到 catch-all 返回
   app 兜底页),不是留一条 502 死路由。
4. **按钮双保险**:mgr-web `SandboxTree.buttonsOf` 的 `INSTALLED_GATE`
   按 mgr 列表 API 的 `installed_services` 过滤四个内置按钮
   (codeServer/vnc/pi/piWeb);`undefined`(老后端)与 `deletable`
   (用户注册)不过滤,Terminal 恒显示。manifest `enabled` 单独不可靠
   (探不倒 ON_DEMAND 特例,老镜像误报)。
5. **services.toml 改动需换 app 镜像**:它是 `include_str!` 进二进制的,
   env-hash 不变时 mgr 会复用旧镜像——须手动 `docker build` 同 tag
   覆盖再 force-recreate app。

**验证点**: 最小沙箱(四服务全关)`GET sbx-<name>.mgr.localhost/api/manifest`
中 codeServer/vnc/piWeb `enabled:false`;其网关 `/code-server/` 返回
app 兜底页而非另一沙箱的 VS Code;全开沙箱 code-server 按需启动后
enabled 翻 true。composegen 单测:caddyfile 无 `reverse_proxy app:`、
路由随安装集裁剪、compose 含 `sbx-{name}-app` 别名。
