# Design — mgr 子域 URL 端口跟随

## 问题本质

浏览器访问 `*.mgr.localhost` 时 URL 是否带端口，决定了它连宿主机的哪个端口：
无端口 → 80。mgr-web 生成的子域 URL 全部无端口，宿主 80 又未必发布，
于是「唯一入口是任意端口」这个架构承诺在子域链路上不成立。

核心设计抉择：**后端不感知端口（它无法知道浏览器从哪个端口进来），
前端统一补偿**。所有 `*.mgr.localhost` URL 在 mgr-web 渲染/打开时附加
`window.location.port`（为空或 80 时不附加，保持现状）。

## 改动点全景（5 处代码 + 1 处部署）

### 1. mgr-web：单一出口函数（核心）

`mgr-web/src/pages/workspace/paneUrl.ts` 新增 `withMgrPort(url)`：

```ts
// 浏览器经宿主端口（如 8081）访问 mgr UI 时，子域 URL 必须带上同一端口
// —— *.mgr.localhost 解析到 127.0.0.1，无端口 = 撞宿主 80（未必发布）。
// 经 80 访问（window.location.port === ""）时保持无端口形式不变。
export function withMgrPort(url: string): string {
  const port = window.location.port;
  if (!port || port === "80") return url;
  return url.replace(/^(https?:\/\/[^/:?#]+)(?=:?\d|[/?#])/,
    (m, origin) => `${origin}:${port}`);
}
```

消费点：
- `sandboxGatewayOrigin()` 返回值套 `withMgrPort`（R1，覆盖 code-server/vnc/
  workbench 所有相对路径面板）。
- `urlFor()` 中 piWeb 绝对 URL（`PI_WEB_URL` 子域形式，已含 `sbx-<name>-
  piweb.mgr.localhost`）套 `withMgrPort`（R2）。`{host}` 替换后的
  `http://<host>:30141` 形式不会被误改——它带显式端口，正则的负向断言
  `(?=:?\d)` 只匹配 origin 后紧跟 `/`、`?`、`#` 或行尾的情况，显式端口
  已存在时 replace 不命中（`http://x:30141/` 中 `[^/:?#]+` 止于 `:`）。
- SandboxListPage / ModelsPage 中 `entry_url` / `piweb_url` 的 `<a href>`
  套 `withMgrPort`（R3 前端侧）。

### 2. 后端：不改语义，仅补注释（R3）

`mgr/src/routes.rs` 三处 `entry_url` / `piweb_url`（:426-427、:516-517、
:881-882）保持无端口形式——它是「容器网络视角的规范形式」（curl Host 头
无端口、宿主 80 直发场景都正确），浏览器端口由前端 `withMgrPort` 补。
加注释说明这个契约，防止后人“顺手修复”回去。

### 3. app 静态指路页跳转目标（R4）

`app/entrypoint.sh:58` 的 sed 目前把 `MGR_PLACEHOLDER_URL` 无条件替换成
写死的 `http://mgr.localhost/`（MGR_URL 的值只当触发器）。改为：
`sed "s|MGR_PLACEHOLDER_URL|${MGR_URL:-http://mgr.localhost/}|g"`，
并在 `mgr/src/composegen.rs:94` 把 `MGR_URL` 从 `http://mgr-api:8089`
改为 `http://mgr.localhost/`——不对：MGR_URL 有双重身份（模型配置 pull
端点 + 跳转目标），不能改值。

正确方案（执行期修正）：MGR_URL 的值是容器内部 pull 端点
（`http://mgr-api:8089`），**绝不能**作为 sed 替换值注入浏览器页面——
原稿「回落 MGR_URL」的回落链有此缺陷。定案：触发器仍是 `MGR_URL`
（set/unset 判断，语义不变），替换值 = `MGR_REDIRECT_URL`（缺省
`http://mgr.localhost/`，composegen 显式注入同值）。宿主端口非 80 的
场景由跳转页 JS 补偿：**跳转页 JS 用当前页面的 port 附加到目标 origin
上**（与 mgr-web `withMgrPort` 同一正则）。该页面在
`sbx-<name>.mgr.localhost:<port>` 下被访问（用户从列表点 entry_url 进来，
entry_url 已被前端补了端口），故 `:port` 即宿主发布端口。`<noscript>` 与
静态 `<a>` 保留无端口形式作为 80 场景回落。

### 4. caddy / 网络 / compose：零改动

总网关域名路由、`sbx-{name}` 别名、容器内端口全不动（见 PRD Out of Scope）。
Caddy Host 匹配忽略端口（实测），子域:8081 请求落到同一 site block。

### 5. 重建链路

mgr-web 改动经 `make mgr-up`（mgr/Dockerfile web-builder 阶段重建）生效。
app 改动（entrypoint.sh / redirect 页）需重建 sandbox-app 镜像并
`--force-recreate` 管理型沙箱。**注意**：既有 sbx-111 的 app 容器用的
是旧镜像，本任务的 AC 不依赖它（AC1-3 走 mgr 代理链路，AC4 只验证
entry_url 打开的指路页能显示——旧镜像无跳转 JS 也显示静态链接）。
完整验证可选重建 sbx-111。

## 数据流（改后）

```
浏览器 http://mgr.localhost:8081/ （唯一入口）
  ├─ UI/API 同源请求 → 总网关 → mgr-api:8089           （不变）
  ├─ code-server 面板 → http://sbx-111.mgr.localhost:8081/code-server/
  │     → 总网关 → sbx-111(容器网关):8080 → app:8200    （端口补上后通）
  ├─ vnc 面板 → http://sbx-111.mgr.localhost:8081/vnc/   （同上）
  ├─ pi-web 面板 → http://sbx-111-piweb.mgr.localhost:8081/
  │     → 总网关 → sbx-111-piweb:30141                   （同上）
  └─ terminal WS → ws://mgr.localhost:8081/api/sbx/...   （本就同源，不变）
```

## 权衡与备选

- **备选 A（被否）**：发布宿主 80。零代码，但占用 80 且与「任意端口唯一
  入口」的产品目标冲突（用户明确要 8081）。
- **备选 B（被否）**：后端 entry_url 按请求 Host 头补端口。mgr-api 收到的
  Host 是 `mgr.localhost`（caddy 反代未改写），拿不到浏览器端口，需 caddy
  传 `X-Forwarded-Port` 之类——引入新依赖面，且 HTML 静态页（redirect 页）
  不走 mgr-api，覆盖不全。
- **备选 C（被否）**：env 配置固定端口（如 PI_WEB_HOST_PORT 模式）。每换
  端口要改 env + 重建，动态取 `window.location.port` 是其超集且零配置。

## 兼容性

- 80 端口访问：`window.location.port === ""` → 所有 URL 无端口，与现状
  完全一致（R5）。
- 容器网络内部 curl（Host 无端口）：不经过 mgr-web，行为不变。
- 旧沙箱（旧 sandbox-app 镜像）：redirect 页无跳转 JS，仍显示静态链接，
  不破坏；重建后获得新行为。

## 测试策略

- 前端：paneUrl 已有单测（`*.test.*` 若存在则扩展；否则新增
  `withMgrPort` 的纯函数测试：带端口/无端口/80/非 mgr 域名/已带端口）。
  jsdom 下 `window.location.port` 可控。
- 后端：routes.rs 的 entry_url 断言不变（无端口形式）。
- e2e 手动：AC1-AC4 按宿主 8081 场景实测（vnc puppeteer 或人工）。
