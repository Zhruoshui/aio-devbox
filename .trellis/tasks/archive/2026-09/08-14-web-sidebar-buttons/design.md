# Design — Web 端侧边栏按钮化改造

## 1. 架构与边界

三处改动，彼此解耦：

1. **前端重写**(`web/src/`)：移除 golden-layout，改为 `左侧边栏 + 主区标签页栈`。面板组件 `IframePane`/`XtermPane`
   复用不变(含 XtermPane 已有的 WS 关闭清理，天然满足“关 tab 即杀 pty”)。
2. **后端 manifest 改判据**(`app/src/config.rs` + `state.rs`)：agent 可见性由 `ENABLE_*` env 改为 command_exists
   (登录 shell PATH 遍历，带缓存)；合并内置 services.toml + 运行时 buttons.toml。
3. **按钮 CRUD**(`app/src/routes/buttons.rs` 新增)：`POST/DELETE /api/buttons` 读写卷上 buttons.toml。

不动的：`/api/term/ws` 通用 pty(无白名单，前端 `?cmd=` 任意)、Caddyfile(catch-all 已透传 /api/* 到 app)、
`pty.rs`、IframePane/XtermPane 的渲染与 WS 协议(5 字节 resize 帧)、`scenarios/*`(opencode category 维持 app)。

## 2. 数据契约

### 2.1 `GET /api/manifest`(扩展，向后兼容加字段)
```json
{ "services": [
  {"id":"codeServer","type":"web","enabled":true,"url":"/code-server/","label":"code-server","deletable":false},
  {"id":"vnc","type":"web","enabled":false,"label":"Chromium","deletable":false},
  {"id":"terminal","type":"agent","enabled":true,"cmd":"","label":"Terminal","deletable":false},
  {"id":"opencode","type":"agent","enabled":false,"label":"opencode","deletable":false},
  {"id":"htop","type":"agent","enabled":true,"cmd":"htop","label":"htop","deletable":true}
]}
```
- `enabled=false` 的项(如未装 opencode、未起 vnc)**仍在数组里**；前端侧边栏只渲染 `enabled=true`，故不出现按钮、
  无死面板。保留全量便于将来“灰显”等扩展。
- 新增 `label`(显示名，缺省回退 id)与 `deletable`(仅用户按钮 true)。

### 2.2 buttons.toml(卷上 `/home/gem/.aio/buttons.toml`)
```toml
[[button]]
id = "htop"        # POST 时生成(slug(label)，冲突加后缀)；DELETE/toggle 的稳定键
label = "htop"
type = "agent"      # 默认 agent；保留字段以备 web 型
cmd = "htop"
```
缺省/不存在 => 空列表(首次 `make up` 空卷)。axum 以 gem 身份 `mkdir -p /home/gem/.aio`(卷属 gem，可写)。

### 2.3 `POST /api/buttons`
- 请求体：`{"label": string, "cmd": string}`(JSON)。
- 校验：label/cmd 去 trim 后非空、长度 ≤64；cmd 非空。失败 400。
- 处理：生成 id(slug(label)，与已存 id 去重) -> 在文件写锁下 read-modify-write 追加 -> 返回 201 + 该按钮。
- 响应：`{"id","label","type":"agent","cmd"}`。
- 信任模型：cmd 任意串(与 `/api/term/ws?cmd=` 同信任级，已有 terminal 按钮即全 shell，无提权面)。

### 2.4 `DELETE /api/buttons/:id`
- 写锁下 read-modify-write 移除匹配 id；返回 204，无匹配 404。

## 3. 后端改动细

### 3.1 `app/src/config.rs`
- `Service` 增 `label: Option<String>`、`deletable: bool`(默认 false 用 `#[serde(default)]`)；**移除 `enable` 字段**
  (serde 默认忽略未知字段，故 services.toml 残留 `enable=` 也不报错，但一并清理)。
- `ManifestEntry` 增 `label: String`(序列化时 `svc.label.clone().unwrap_or_else(|| svc.id.clone())`)、
  `deletable: bool`。
- 删 `is_agent_enabled`；`build_manifest` 签名改为 `async fn build_manifest(services: &[Service], cache: &PathCache)`
  —— agent 分支改 `command_exists(&svc.cmd, cache)`；web 分支不变。所有 agent 探测用 `futures::join` 并行
  (本进程内遍历，快)；web TCP 探活仍 400ms 并行。
- 新增 buttons.toml 解析：`pub fn load_buttons(path: &Path) -> Vec<Service>`(文件缺/空 => 空 vec；解析错 =>
  空 vec + tracing::warn，不炸 manifest)。每按钮 `id/label/type=Agent/cmd`，`deletable=true`。

### 3.2 command_exists 实现(无 `which` 依赖)
- `PathCache { dirs: Vec<PathBuf>, fetched_at: Instant }`，TTL 60s。
- `resolve_login_path()`：`tokio::process::Command::new("bash").arg("-lc").arg("printf %s \"$PATH\"")` 取 stdout，
  `std::env::split_paths` 解析。失败回退 `std::env::var("PATH")`(覆盖 /usr/local/bin 烘进工具)。
- `command_exists(cmd, cache)`：`cmd.trim()` 空 => true(terminal)；否则在缓存 dirs 里查 `dir.join(cmd).is_file()`。
- 缓存放 `AppState`(`Arc<RwLock<PathCache>>`)；`build_manifest` 借读，过期则重解析(写锁)。
- 局限：shell 函数/别名不探测(rare；可接受；记 design §7)。PATH 与 pty 同走 `bash -l`，解析一致。

### 3.3 `app/src/state.rs`
```rust
#[derive(Clone)]
pub struct AppState {
    pub builtin: Arc<Vec<Service>>,            // services.toml，启动 load_services()
    pub buttons_file: PathBuf,                  // env AIO_BUTTONS_FILE 或 /home/gem/.aio/buttons.toml
    pub path_cache: Arc<RwLock<PathCache>>,
    pub file_lock: Arc<Mutex<()>>,              // buttons.toml read-modify-write 串行
}
```
(`Mutex` 用 `std::sync::Mutex` 配 `spawn_blocking`，或 `tokio::sync::Mutex`；文件小，取 std + spawn_blocking。)

### 3.4 `app/src/routes/buttons.rs`(新增)
- `POST /api/buttons`：解析 body -> 校验 -> 取 `file_lock` -> read buttons.toml -> 生成 id -> 写回(原子：
  写临时文件再 rename，避免半写) -> 201。成功后**不**主动刷 manifest；前端收到 201 自行重取 manifest。
- `DELETE /api/buttons/:id`：Path extractor `:id` -> 锁 -> read-modify-write -> 204/404。
- 在 `main.rs` 注册于 `/api/*rest` catch-all 之前(静态路由优先)。

### 3.5 `app/src/routes/manifest.rs`
- handler 读 `state.buttons_file` 的 buttons + `state.builtin` 合并(去重 by id：内置优先，同 id 用户按钮忽略
  + warn) -> `build_manifest(&merged, &state.path_cache).await`。

### 3.6 `app/src/main.rs`
- 构造新 AppState；router 增 `POST /api/buttons`、`DELETE /api/buttons/:id`。

### 3.7 `app/services.toml`
- agent 记录删 `enable`；所有记录加 `label`(codeServer→"code-server"、vnc→"Chromium"、terminal→"Terminal"、
  opencode→"opencode")。

### 3.8 `docker-compose.yml`
- app.environment 移除 `ENABLE_TERMINAL`/`ENABLE_OPENCODE`(及其注释块)。

## 4. 前端改动细(`web/src/`)

### 4.1 新结构
- `App.tsx`：fetch manifest；state = `manifest`、`openTabs: string[]`(有序 id)、`activeTab: string|null`、
  `sidebarCollapsed: bool`(localStorage)。terminal 若 enabled 则首次 openTabs=`["terminal"]`、active=terminal。
  manifest 刷新后：`openTabs` 过滤掉新 manifest 里没有的；不主动加新按钮。
- `Sidebar.tsx`(新)：渲染 `manifest.filter(s=>s.enabled)`，每项一个 toggle 按钮(在 openTabs 中则高亮/active)；
  顶部“☰”折叠 + “↻”刷新；底部“+ 注册按钮”表单(本地 state label/cmd -> POST -> 刷新)；用户按钮(`deletable`)
  悬停显“✕”删除(DELETE -> 刷新)。折叠态收成图标轨。
- `TabStack.tsx`(新)：tab 栏(openTabs 顺序，每 tab 显 `label` + “✕”关、点切 active) + 主区渲染 active 对应面板。
- `PaneForService`(App 内或独立)：`type==="web"` -> `IframePane`，否则 `XtermPane`(不变)。
- `IframePane.tsx` / `XtermPane.tsx`：保留。XtermPane 清理已关 WS+dispose(`:111-116`)，toggle 关 tab 即卸载 ->
  cleanup -> WS close -> pty 退出(R2c 满足)。顺手把过时的“Phase E”文案(`:97`)改成中性提示、保留 1 次重连。
- `types.ts`：`ServiceEntry` 增 `label: string`、`deletable: boolean`。
- `layout.ts`：**删除**。`package.json`：移除 `golden-layout` 依赖。`main.tsx`：去 golden-layout CSS import。
- `styles.css`：重写为 `sidebar + tabs` 布局(侧栏宽 ~48px 折叠/~200px 展开、tab 栏、主区 flex:1)。

### 4.2 交互
- 点侧边栏按钮：id 在 openTabs => 移除(关 tab；若 active 被关，active 切相邻)；否则 push 并设 active。
- tab “✕”：同上移除。点 tab：设 active。
- “↻”刷新：重 fetch manifest。窗口 focus 也触发一次(去抖)。
- “+ 注册按钮”：表单提交 -> `POST /api/buttons` -> 成功后刷新 manifest(新按钮以 enabled 出现，因 command_exists
  即时探测)。

## 5. 兼容与迁移
- 移除 `enable` 字段：serde 默认不 `deny_unknown_fields`，旧 services.toml 残留 `enable=` 不报错；仍清理之。
- golden-layout 移除：`web/package.json` 删依赖、`layout.ts` 删、App 重写；`npm ci` 后 bundle 无 GL。
- buttons.toml：全新文件，无迁移；旧 app 不读它(无害)。
- 回滚：revert 提交即可；卷上 buttons.toml 残留不影响旧 app。

## 6. 取舍
- **command_exists 走登录 shell PATH(非 env)**：准(含 ~/.local/bin 运行时补装)，代价 = 一次 bash 启动(缓存摊销)。
  备选“构建期场景元数据”无法反映运行时补装，否决。
- **buttons.toml 卷上文件(非 localStorage/非 services.toml)**：跨重建+多端共享+有 UI 入口；代价 = 一对
  POST/DELETE 端点 + 文件写锁。
- **每按钮单 tab · toggle(非多开)**：贴合用户“展开/收起”表述、实现简；代价 = 多终端需多注册按钮(OOS)。
- **manifest 不轮询(手动↻+focus)**：简单；代价 = 运行时补装后需点一下刷新(可接受，低频)。

## 7. 已知局限 / 风险点
- command_exists 不识别 shell 函数/别名(仅 PATH 上可执行文件)；罕见，记之。
- XtermPane 在 pty 自行退出(用户在 opencode 里 exit)时会重连一次重开新会话(既有行为，非本任务引入)；如需
  “退出即关 tab”属后续增强，OOS。
- app 以 gem 写卷上文件：`/home/gem/.aio` 首次需 mkdir；gem 对 /home/gem 有写权，OK。
- buttons.toml 并发写：`file_lock` 串行 + 原子 rename；读(manifest)与写并发时读到旧/新整个文件，不致半文件。
- command_exists 首请求延迟：PATH 解析 ~100ms + 探测(缓存命中后纯内存)；可接受，且 manifest 非高频。

## 8. 验证手段
- `cargo test`(新增 buttons 解析/去重、command_exists 空 cmd、label 回退)。
- `cd web && npm run build`(tsc + vite，无 GL 残留)。
- `make up` 实跑：AC1-AC9 手测(侧边栏按钮显隐、toggle、注册/删除、重建持久)。
