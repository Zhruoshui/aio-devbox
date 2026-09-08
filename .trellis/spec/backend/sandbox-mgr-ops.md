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
- mgr 依赖 aio-config **lib**(scenario/gen),但**不依赖** aio-models
  (Phase 4 才需要)——Dockerfile 的 dep-cache 层与真实源层都不要拷
  aio-models/src,Phase 4 接入时同步加。
- 依赖路径 crate 的 lib 时,真实源层的 touch 清单必须包含其 `lib.rs`
  (BuildKit COPY-mtime 陷阱见
  [CI Image Conventions 约定 7](../guides/ci-image-conventions.md));
  dep-cache 层必须为**每个** workspace member 写 dummy 源,漏一个
  `cargo build -p` 直接报 target resolution error。
