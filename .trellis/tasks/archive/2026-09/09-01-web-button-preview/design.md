# Design — 用户注册 web 类型按钮 + /preview 动态反代

## 1. 现状与接入点

- 数据模型:`app/src/config.rs` 的 `ButtonDef { id, label, button_type, cmd }`,serde 注释早已预留 `"web"` 类型;`load_buttons()`(config.rs:234)把 web 映射到 `ServiceType::Web`,但 target/url 硬编码 `None`。
- CRUD:`app/src/routes/buttons.rs` 的 `create_button` 硬编码 `button_type: "agent"`,`ButtonInput` 只有 `label`+`cmd`。
- 显隐:`build_manifest`(config.rs:268)对 `ServiceType::Web` 走 `is_web_reachable`(TCP 400ms 探活 `target`);`url` 字段经 `ManifestEntry.url` 下发,IframePane 直接渲染。
- 路由:`app/src/main.rs` axum Router,`/api/*` 静态段 + `seam` 兜底 + `fallback_service(ServeDir)`;`/preview/*` 目前落进 ServeDir(SPA fallback),新增静态路由即可接管,main.rs 已验证 "静态段排名高于 catch-all" 机制。
- 网络拓扑:dev server 在 app 容器的终端(pty 由 app 以 root spawn)里启动 → 绑定 app netns 的 loopback → **axum 从 127.0.0.1:<port> 直达**,无需跨容器。gateway catch-all(`handle { reverse_proxy app:8088 }`)已把 `/preview/*` 交给 axum。

## 2. 方案决策:axum 侧自建反代(issue 方案 2)

| | 方案 1:Caddy 通配 | 方案 2:axum handler(选定) |
|---|---|---|
| 动态端口路由 | Caddyfile 需要 `path_regexp` 捕获 + `{http.regexp.*}` 占位符做 dynamic upstream,写法脆、难测 | matchit `:port` 路径参数,Rust 内直接解析校验 |
| 网络可达 | 需走 `app:<port>`(sandbox-net DNS),要求 dev server 绑 0.0.0.0(默认多绑 127.0.0.1,会踩坑) | 127.0.0.1 直达,与 dev server 默认绑定习惯一致 |
| WS/SSE | caddy 自动,但与 axum 方案无差 | reqwest 流式转发 + WS upgrade 双向泵 |
| 改动面 | gateway/Caddyfile + compose(违背 PRD 约束) | 仅 app + web |

netns 疑虑澄清:issue 担心 "gateway 从自己 netns 无法直达 dev server" —— 实际 dev server 在 app netns 内,gateway 以 `app:<port>` 可达;但要求 dev server 绑 0.0.0.0,而 axum 方案对绑 127.0.0.1 也成立,更稳。

## 3. 数据模型与 API 契约

### buttons.toml(ButtonDef 扩展)

```toml
[[button]]
id = "my-vite"
label = "my vite"
type = "web"        # 缺省仍为 "agent"
cmd = ""            # web 类型存空串(字段保留,向后兼容)
port = 5173         # 新增,Option<u16>,web 必有;agent 序列化时省略
```

- `ButtonDef` 增加 `#[serde(default, skip_serializing_if = "Option::is_none")] pub port: Option<u16>`:旧文件(无 port)反序列化不受影响;agent 按钮写盘不带 port 字段(文件 diff 最小)。

### POST /api/buttons(ButtonInput / ButtonOut 扩展)

```jsonc
// 请求(web)
{ "label": "my vite", "type": "web", "port": 5173 }
// 请求(agent,完全向后兼容)
{ "label": "htop", "cmd": "htop" }
// 响应统一: { id, label, type, cmd, port? }  (port 仅 web 携带)
```

校验规则(在现有 label 长度/非空校验之上):

| 情形 | 结果 |
|---|---|
| type=web,port 缺失/非 1–65535/== 8088 | 400,信息指明原因 |
| type=web,cmd 非空 | 忽略 cmd(存空串) |
| type=agent,cmd 空 | 400(现状) |
| type 未知值 | 400 |

8088 自环防护:反代到 axum 自身会形成 `/preview/8088/preview/...` 递归与 SPA 自反代混淆,拒绝注册(传输层同样拦截,见 §4)。

### manifest 下发(load_buttons 改动)

web 按钮 → `Service { service_type: Web, target: Some("127.0.0.1:<port>"), url: Some("/preview/<port>/"), cmd: None, deletable: true }`。`build_manifest` / `is_web_reachable` / 前端 `IframePane` 全部复用,零改动。

## 4. /preview 动态反代(app/src/routes/preview.rs,新文件)

### 路由注册(main.rs,置于 seam 路由组旁)

```rust
.route("/preview/:port", any(preview_proxy))
.route("/preview/:port/", any(preview_proxy))
.route("/preview/:port/*path", any(preview_proxy))
```

(三条同 main.rs 既有注释:matchit 0.7.3 catch-all 不匹配空尾段。)

### 处理流程

1. **入参归一**:解析 `:port` 为 u16;非法、0、8088 → 404(快速失败,不反代)。
2. **上游路径**:转发路径 = `/` + `*path`(strip `/preview/<port>` 前缀,`handle_path` 同语义)+ 原样 query。
3. **WS 分流**:请求含 `connection: upgrade` + `upgrade: websocket` → WS 通路(§4.1);否则 HTTP 通路(§4.2)。
4. **HTTP 通路(reqwest,复用 AppState::http 语义但独立 client,见 §6)**:
   - 转发 method / headers(剥离 hop-by-hop:`connection, keep-alive, proxy-authenticate, proxy-authorization, te, trailer, transfer-encoding, upgrade`)/ body(bytes)。
   - Host 头改为 `127.0.0.1:<port>`(loopback 反代常规语义;pi-web 的 IP-literal 信任已验证此路可用)。
   - 上游响应:状态码 + headers(hop-by-hop 同剥)+ **流式 body**(`bytes_stream()` → `axum::body::Body::from_stream`),SSE / chunked 不缓冲。
   - 连接拒绝/超时 → 502(与 seam 同语义)。
5. **WS 通路(tokio-tungstenite)**:
   - `connect_async("ws://127.0.0.1:<port><path-and-query>")`,透传 `sec-websocket-protocol` 子协议。
   - 服务端 `WebSocketUpgrade::on_upgrade` 拿到客户端半程后,两个 task 双向泵(client↔upstream 消息级转发),任一侧关闭/出错即终止另一侧。
   - 不引入 rustls:upstream 恒为 127.0.0.1 明文 ws。

### 已知边界(README 记录,不做服务端 HTML 改写)

- 根绝对资源(`/@vite/client`、`/_next/...`)在子路径下天然断裂 —— 与 pi-web 需要 dedicated origin 同根因。vite 项目以 `base: '/preview/<port>/'` + `server.hmr.path: '/preview/<port>/'` 一行配置即完整工作(HMR ws 落到 `/preview/<port>/` 被本反代接管);纯相对资源 server(python -m http.server 等)零配置可用。
- **不做**响应体 HTML 重写/`<base>` 注入:regex 改写 HTML 对 JS 动态拼 URL 无效,产生半吊子兼容,维护成本 > 收益;与 code-server `/proxy/` 同类方案的取舍一致。

## 5. 前端改动

- `web/src/RegisterDialog.tsx`:新增类型 segmented control(agent=命令 / web=端口预览);web 态渲染端口输入(inputmode=numeric),客户端校验 1–65535 且 ≠8088;文案走 i18n(zh/en)。焦点管理、Escape、错误呈现沿用现有 dialog 模式。
- `web/src/App.tsx`:`registerButton(label, cmd)` → `registerButton(input: RegisterButtonInput)`,body 按 type 组装。
- `web/src/types.ts`:补 `RegisterButtonInput`(API 请求镜像)。manifest 侧 `ServiceEntry` 无需变更(url 语义不变)。
- 显隐/删除:`fetchManifest` 轮询与 `deleteButton` 现有逻辑直接覆盖 web 按钮,零改动。

## 6. 依赖与配置

- 新增 crate:`tokio-tungstenite = "0.23"`(仅 plaintext,`default-features = false`)。reqwest/axum/ws 均已具备。
- 独立反代 reqwest client:AppState::http 无全局超时,可直接复用;若实现中发现需要差异配置(如禁用自动解压,保证 SSE 语义干净——reqwest 默认不自动解压,确认无需)则再拆,不为拆而拆。

## 7. 兼容与回滚

- buttons.toml 旧文件、旧 agent 注册路径(前端/后端)全兼容;单测覆盖 "旧格式文件读取"。
- 回滚 = revert 提交即可:`/preview` 路由与 port 字段均为增量,无数据迁移。

## 8. 测试设计

- Rust 单测(config.rs / buttons.rs / preview.rs):
  - load_buttons:web 按钮 → Web + target/url 正确;旧格式(无 port)agent 按钮不受影响;
  - create_button 校验矩阵:web 无 port / port=0 / port=65536 / port=8088 / 未知 type → 400;agent 路径不回归;
  - 纯函数:strip 前缀路径拼接、hop-by-hop 剥离表。
- 手工验收(make up):vite dev(`base` 配置后)预览 + HMR;SSE(如 `sse-test` 端点)不断流;无监听端口按钮隐藏;删除生效。
