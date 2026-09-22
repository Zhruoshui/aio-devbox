# design.md — picker UX（折叠/全选 + 镜像详情 + 构建日志）

> 范围（D-a）：mgr-web + mgr 后端。config TUI 不动。

## 0. 触点地图

| 关注点 | 前端 | 后端 |
|---|---|---|
| 层折叠/全选 | `mgr-web/src/pages/EnvPicker.tsx` | — |
| 镜像详情 | `mgr-web/src/pages/ImagesPage.tsx`、`types.ts`、`api.ts`（透传） | `mgr/src/db.rs`（新列）、`mgr/src/jobs.rs`（写入）、`mgr/src/routes.rs`（响应） |
| 构建日志 | `mgr-web/src/pages/JobView.tsx`（无需改，轮询即可） | `mgr/src/docker.rs`（流式 build）、`mgr/src/jobs.rs`（双 cap + stderr） |

i18n：`mgr-web/src/i18n.ts`（中/英双语，模式照现有 `layL*/ins*` 键）。

## 1. EnvPicker 层折叠 + 全选

纯前端、纯局部 UI 状态，不动 `SandboxEnv` 契约与 `defaultEnv`。

### 1.1 状态

```ts
const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
```

key = layer 的 `labelKey`（L1-L4 + `layOther`）。默认全展开（空 record）。

### 1.2 层头渲染

现有 `group()` 只渲染 `<p className="layer">`。改为：

```tsx
<div className="layer-head">
  <button className="layer-toggle" aria-expanded={!isCollapsed}
          onClick={toggle collapse}>
    <Icon chevron /> {t(lang, labelKey)}
    <span className="layer-count">{selected}/{total}</span>
  </button>
  <button className="btn btn-link layer-all" onClick={selectAllNone}>
    {allSelected ? t(lang, "layerNone") : t(lang, "layerAll")}
  </button>
</div>
{!isCollapsed && layerRows(items)}
```

- **计数**：`total` = 层内非 always_on 项数；`selected` = 其中已勾选数。always_on 不参与
  计数（它们永远装、无选择意义），避免「必装 3 项」干扰「我选了几项」。
- **全选/全不选**：目标是「该层非 always_on 项全集」。判定 `allSelected` = 层内每个
  非 always_on 项都在 `env.scenarios`。点击时：
  - 未全选 → 并集上全集（已在的保持，缺的补上）；
  - 已全选 → 从 `env.scenarios` 移除该层全部非 always_on id。
  - always_on 行不进 `env.scenarios`（契约：API 拒绝 always_on id），故天然被排除。

### 1.3 折叠与提交的无关性

折叠只影响渲染分支，提交体仍是 `env`。AC1 即「折叠态改变前后 `onChange` 产出一致」。

### 1.4 CSS

`components.css` 已有 `.layer`/`.rows`/`.row`。新增 `.layer-head`（flex 行，toggle 左、
全选按钮右）、`.layer-count`（dim）。复用现有 `.btn-link`（若缺则补一个无框文字按钮样式），
不引入新色系。

## 2. 镜像详情（结构化 env）

### 2.1 schema 迁移（db.rs）

照 `combo` 列同款 pragma-probe 模式：

```rust
// images 表新增两列（SCHEMA 常量同步加，老库走 ALTER 分支）：
env_json TEXT,        -- R3: 构建时 SandboxEnv 规范化 JSON（scenario 详情源）
services_json TEXT,   -- R3: Services {code_server, vnc} JSON；pi/pi-web 由 env.scenarios 派生
```

`db::upsert_image` 增参 `env_json`/`services_json`；`''`（空串）= 该次未提供（沿用
`build_log` 的 `CASE WHEN ?x = ''` 保留旧值语义，本任务总是提供）。

`list_images` 返回元组扩为 7 字段（env_hash, tag, built_at, build_log, combo,
env_json, services_json）。`db.rs` 内三处用到该元组的测试同步更新。

### 2.2 create job 写入（jobs.rs）

`run_create` 中 `db::upsert_image` 调用处补：

```rust
db::upsert_image(&conn, &hash, &base_tag, &base_build_log, Some(&combo),
                 &env.canonical_json(), &services.canonical_json())
```

`services.canonical_json()` 已存在（db::Services 的写入侧即用它）。

### 2.3 API（routes.rs list_images）

每行 JSON 增两键：

```json
"env": { "scenarios": [...], "versions": {...} },        // env_json 解析；空串 -> null
"services": { "code_server": true, "vnc": false }        // services_json 解析；空串 -> null
```

解析失败（脏数据）时给 `null`，前端回退——「never break on the unknown」。

### 2.4 前端 types.ts

`Image` 增：

```ts
/** R3: structured build-time env; null on rows recorded before R3. */
env: SandboxEnv | null;
/** R3: build-time service switches; null on pre-R3 rows. */
services: { code_server: boolean; vnc: boolean } | null;
```

`api.ts` 不加新函数（`listImages` 透传，`get<T>` 直接 as T——与现有 Image 解码一致，
decode-once-at-boundary 惯例下 `Image` 类型即契约）。

### 2.5 ImagesPage 详情展开

复用现有「行内展开」模式（与 build-log 同一行展开位，Q1 按倾向定）：

- combo 单元格从 `<td title=...>` 改为 `<button className="combo-toggle">`，
  `aria-expanded`，点击切换 `openDetail === img.env_hash`。
- 展开行 `colSpan={8}`，与 build-log 展开**共用同一展开行**（一个 hash 同时只展开一个
  面板：点 combo 显示详情，点 log 显示日志，再点关闭——单一 `open` state，值域改为
  `"" | "combo:<hash>" | "log:<hash>"`，避免两个独立 state 抢一行）。
- 详情区结构：

```
组合详情                     （env_hash 短码）
场景：rust · 1.93.1（默认）   <- 选中项，显式版本标「显式」否则「默认」
      go · 1.23.4
必装：node 22.x · python 3.12 · mise（always_on 行 + 其 versions）
服务：code-server ✓  vnc ✗  pi ✓  pi-web ✗
```

- 渲染数据装配：前端拿到 `img.env`（scenarios id 数组 + versions map）与
  `img.services`，再对照页面已持有的 `scenarios` catalog（ImagesPage 当前没有——
  需新增一次 `listScenarios()` 拉取，或在 App 层传入；取**页面自拉**（`useEffect`
  一次），与 ImagesPage 现有自拉 `listImages` 一致）。
- always_on 段：catalog 里 `always_on` 且不在 SERVICE_SCENARIOS 的项 +
  `img.env.versions[id]`。pi/pi-web 归属服务区，不重复出现在场景段（S1 单一开关归属）。

## 3. 构建日志增强

### 3.1 docker.rs 流式 build

`docker::build` 现状：`run_capture` 一次 `output()`，构建期无任何输出。新增：

```rust
/// Streamed build: spawn `docker build`, forward stdout+stderr LINE-BY-LINE to
/// `on_line` as they arrive; returns full captured stdout on success.
/// On failure the error carries the merged tail (stdout+stderr) so the caller
/// can append it to the job log (R4.3).
pub async fn build_streaming(
    context: &Path, dockerfile: &Path, tag: &str,
    build_args: &[(&str, &str)],
    on_line: &mut dyn FnMut(&str),
) -> Result<String>
```

实现要点：
- `tokio::process::Command`，`stdout(Stdio::piped())` + `stderr(Stdio::piped())`，
  两个 reader 用 `tokio::io::BufReader::lines()`，经 `tokio::select!` 合并读——
  docker build 进度走 **stderr**（BuildKit），普通输出在 stdout，**必须合并**否则
  丢一边。
- 每行回调前 `trim_end`；空行跳过。
- 全程另存 `String` 累积（带 cap，如 1MB），success 返回它（供 build_log 落库），
  failure 以它为上下文造 anyhow error。

`build`（旧签名）保留——`composegen`/镜像删除等路径不用它，但保留兼容测试与潜在调用；
实际上全仓只有 `jobs.rs` 调 `build`，可直接替换签名，不留旧的（YAGNI，改动面更小）。
**决策：替换 `build` 为流式版**，原 `run_capture` 继续服务非构建命令。

### 3.2 jobs.rs 双 cap + 流式接线

```rust
/// Job-view tail while running (small: the poll transport re-sends it每1.5s).
const LOG_TAIL: usize = 8 * 1024;
/// Images-table build_log tail (R4.2: bigger留存量，覆盖失败现场).
const BUILD_LOG_TAIL: usize = 256 * 1024;
```

- `append_log` 仍裁到 `LOG_TAIL`（运行期）。
- `run_create` 的 base 构建改用 `build_streaming`，回调里 `append_log(&log, line)`。
  `base_build_log = tail(&captured, BUILD_LOG_TAIL)`。
- app/cs/vnc 三个构建同样改流式（同回调），但它们的输出不落 images 表。
- **失败路径**：`build_streaming` 返回 Err 时，jobs.rs catch 后把 error 里的 merged
  tail 追加进 job log（R4.3），再返回 Err 走原有 `job.error` 通道。
- **执行期补充（AC4 倒逼）**：失败构建原本在 images 表无任何痕迹（upsert 只在
  全部构建成功后的主路径执行）。现于 base 构建 Err 分支补记一行：
  build_log = 失败输出 tail、combo/env_json/services_json 照常写入、built_at =
  尝试时间。该 refcount=0、size 读 "—"，cleanup/删除路径天然兼容（rmi 跳过
  缺失镜像）。

锁竞争（Q3）：append_log 每行拿一次 `TokioMutex<JobShared>`。构建期 docker 行率约
10-100 行/秒峰值，持锁仅 `push_str` + 条件截断，微秒级——**逐行可接受**；若实测热点再
改批量。定：**逐行**。

### 3.3 JobView 前端

不改。轮询 1.5s + 现有 `scrollTop = scrollHeight` 已能呈现滚动日志。

## 4. 契约与兼容

| 模块 | 改动 | 兼容 |
|---|---|---|
| `db.rs` images 表 | +2 列（pragma-probe） | 老库自动 ALTER；老行两列 NULL/空 → API 给 null |
| `db::upsert_image` | 签名 +2 参 | 调用点仅 jobs.rs 一处 |
| `routes.rs list_images` | 响应 +env/services 键 | 旧 mgr-web 收到多余键无副作用 |
| `types.ts Image` | +2 字段 | 后端空值 → null，前端 `?? null` 回退 |
| `docker.rs build` | 改流式（签名 +on_line 回调） | 仅 jobs.rs 调用 |
| EnvPicker | 纯 UI 状态 | SandboxEnv 契约不变 |
| ImagesPage | 展开位单一 state 改造 | 行为兼容（log 展开照旧可用） |

无破坏性变更；不迁移历史数据（D-b）。

## 5. 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| docker build 行率突增（progress 刷屏） | job log 频繁截断、锁热 | append_log 已有 8KB cap；截断是 O(len) memcpy，8KB 下可忽略；行率实测超标再改批量 flush |
| 双 reader select! 一路先 EOF | 另一路残留未读 | select! 循环到两路都 EOF；`read_line` 返回 0 = EOF 计数，2 路齐退 |
| images 表行变大（256KB log + env） | list_images 传输体变大 | 个人规模（行数 < 100），百 KB/行可接受；不引入分页 |
| EnvPicker 全选误触 | 用户多选了不想要的大件（rust 1.4G） | 全选按钮二次确认？**不做**——编辑页可再取消，创建向导提交前可回看；保持轻量 |
