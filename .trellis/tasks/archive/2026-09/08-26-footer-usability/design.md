# Design — Statusbar 页脚可用性功能增强

## 0. 决策记录(来自 brainstorm)

| 决策 | 结论 | 依据 |
|------|------|------|
| MVP 范围 | A1 布局持久化 + B1 资源监控 + A4 状态真实化 + A3 地址复制 | 用户选型 |
| 资源视角 | **仅容器视角**(cgroup v2) | 用户拍板:宿主可能是 Windows/Linux,容器语义统一 |
| 布局存储 | localStorage(不做后端漫游) | 仓库惯例:`aio.theme`/`aio.lang`/`aio.sidebar.collapsed` |
| 展示形态 | 纯文本紧凑读数,无图表/面板 | 26px 页脚 + Kumo 极简风格;图表另开任务 |

## 1. 架构与边界

```
app/(Rust axum)                          web/(React SPA)
┌─────────────────────┐   GET /api/stats  ┌──────────────────┐
│ tokio 后台采样任务    │ ◄──3s 轮询────── │ useStats() hook   │
│ 2s 周期读 cgroup/stat │                  │ (App.tsx 持有)     │
│ → AppState.stats     │ ───JSON 响应────► │  ├ StatsSnapshot  │
└─────────────────────┘                  │  └ online: bool   │
                                          │      │ props      │
golden-layout ──stateChanged──► localStorage │    ▼
(aio.layout, debounce 500ms)            │ Statusbar(纯展示) │
                                          └──────────────────┘
```

- Statusbar 保持纯展示(App.tsx 注释契约);所有 fetch 收敛到新 hook `useStats`,在 App.tsx 调用后经 props 下发。
- 弹出子窗口(SUB_WINDOW 分支)不渲染页脚、不启动 useStats,维持现状。
- golden-layout 布局持久化逻辑放 App.tsx 的 GL effect 内(它已是 GL 实例唯一 owner)。

## 2. API 契约 — GET /api/stats

响应(`app/src/routes/stats.rs` 为序列化唯一 owner,`web/src/types.ts` 镜像):

```json
{
  "cpuPct": 12.4,
  "memUsedBytes": 1300000000,
  "memTotalBytes": null,
  "diskUsedBytes": 8500000000,
  "diskTotalBytes": 62000000000
}
```

- `memTotalBytes: null` 表示 cgroup 未设内存上限(compose 现状);前端降级为只显示绝对用量。
- CPU 为 0–100 的容器占用率(相对有效配额,见 §3)。
- 语义:**容器自身视角** —— CPU/mem 取 cgroup v2 `/sys/fs/cgroup/`,disk 取工作区卷挂载点 `/home/gem`(compose `workspace:/home/gem`)的 statvfs。

## 3. 后端采样任务(app/)

- `AppState` 增加 `stats: Arc<RwLock<StatsSnapshot>>`;`main.rs` 启动时 `tokio::spawn` 周期任务(2s),handler 只读快照 → 无锁竞争热点。
- **CPU%**:`cpu.stat` 的 `usage_usec` 是累计值,单次读无法得速率。采样任务维护上次 `(usage_usec, instant)`,本次 pct = `Δusage / (Δt × cpus_eff)`。
  `cpus_eff`:`cpu.max` 非 `max` 时取 `quota/period`;否则 `std::thread::available_parallelism()`。首次采样(无增量)返回 0。
- **MEM**:`memory.current` − `memory.stat` 的 `inactive_file`(docker stats 同款口径,剔除页缓存虚高;无页缓存可言时该值为 0)。`memory.max` 为 `max` → total = null。
- **DISK**:statvfs(`/home/gem`)。依赖新增 `nix`(仅开 `fs` feature)调 `statvfs`;used = `f_blocks − f_bfree`。
- 文件读取失败(非 cgroup v2 环境等):该字段保持上次值并 `tracing::warn!` 一次;首次失败给 0/null,端点恒 200(页脚是装饰性信息,不值得 5xx)。

## 4. 前端 useStats hook(web/src/useStats.ts)

- `useStats(): { stats?: StatsSnapshot; online: boolean }`,`setInterval` 3s `fetch("/api/stats")`;卸载时清 interval + AbortController。
- **A4 借道**:stats 轮询天然是后端心跳 —— 任一次 stats fetch 失败 → `online=false`,成功 → `true`。初始值由 manifest 首载结果播种(loading 阶段无红点闪变)。
- 采样(后端 2s)与轮询(前端 3s)周期互质,避免锯齿同步显示。

## 5. 布局持久化(A1)

- **保存**:GL effect 里 `gl.on("stateChanged", scheduleSave)`;`scheduleSave` 500ms 防抖后执行
  `localStorage["aio.layout"] = JSON.stringify(ResolvedLayoutConfig.minifyConfig(gl.saveLayout()))`。
  minify/unminify 往返在 popout 路径(`consumeSubWindowLayout`)已有先例,直接复用同一套 API。
- **恢复**:GL 初始化 effect 中,构建默认布局前先读 `aio.layout`:`unminifyConfig → LayoutConfig.fromResolved → 覆写 dimensions.headerHeight=40 → loadLayout`;任何一步失败(catch)落回现有默认单 terminal 路径。
- **seq 重同步**:恢复后遍历 saved config 的 componentState,取每个 service 的 `max(seq)` 回填 `seqRef`,否则恢复后再开 terminal 会从 "(2)" 重新计数撞标题。
- **重置**:页脚 icon-btn → `localStorage.removeItem("aio.layout")` + `window.location.reload()`。reload 走现成默认布局路径,确定性强,不做 GL 热重建。
- **已知局限(接受)**:popout 子窗开着时父窗保存的 layout 可能带 popout 引用,restore 失败会 catch 落回默认 —— 安全但有损;记入 spec 后续任务。

## 6. Statusbar 改版(web/src/Statusbar.tsx)

布局(左→右,复用 `.seg`/`.mono`):

```
[● N 个服务可用 · ids]  [CPU 12% · MEM 1.2G · DISK 8.5G]  [host:8080 ⧉] [⟲] [◐] [🌐]
```

- dot:`.dot.ok / .dot.down` 两态(新增 CSS 变量色);`online=false` 时文案切 `statusOffline`。
- stats seg:`title` 提示"CPU / 内存 / 磁盘(容器视角)";`stats === undefined`(后端不可达/首帧未到)时整段隐藏 —— R2.3 静默降级。
- host 区块变 `<button>`:复制 `window.location.origin + "/"`。**secure context 陷阱**:局域网 `http://192.168.x.x:8080` 下 `navigator.clipboard` 为 undefined,必须有隐藏 textarea + `document.execCommand("copy")` 降级。成功后按钮文本切 `copied`("已复制")1.5s 回弹。
- 新 icon-btn:复制(⧉)、重置布局(⟲);`icons.tsx` 补 sprite。
- i18n 新 key(zh-CN/en 同步):`statusOffline`、`copied`、`resetLayout`、`statsTip`。

## 7. 兼容与回滚

- /api/stats 是纯新增端点,不动 manifest/term/buttons 契约;前端 types.ts 只增不改(ServiceEntry 不动)。
- localStorage 键新增 `aio.layout`,旧值缺失 = 首次访问行为不变。
- 回滚单元 = 单 commit revert;无数据迁移。风险文件:`app/src/state.rs`(AppState 加字段,注意 `AppState::new` 同步改)、`web/src/App.tsx`(GL effect 改动,注意 popout 分支不回归)。

## 8. 验证

- `cd app && cargo check`(后端编译门)
- `cd web && npm run build`(tsc --noEmit + vite,前端类型门)
- `make up`(`up: build-base` 会重编 app 镜像:web-builder 阶段 + cargo 都在 app Dockerfile 内)
- 冒烟:`docker run --rm --network host -v "$PWD/web":/web -w /web aio-smoke node smoke-test.cjs`(现有 6 断言无回归)+ 新增断言见 implement.md
- 手动 AC:刷新恢复布局、重置、复制(含 http 非 secure context)、`docker stats` 对拍内存读数
