# Implement — Statusbar 页脚可用性功能增强

执行顺序:后端 → 前端 → 集成验证。每步有独立验证点,后端与前端(1/2 步)无相互依赖可并行。

## 1. 后端 /api-stats(app/)

- [x] 1.1 `app/Cargo.toml`:加 `nix = { version = "0.29", features = ["fs"] }`(statvfs 用)
- [x] 1.2 `app/src/routes/stats.rs`(新):`StatsSnapshot` 结构(`cpuPct: f64`,`memUsedBytes: u64`,`memTotalBytes: Option<u64>`,`diskUsedBytes/diskTotalBytes: u64`,Serialize + Clone);`stats()` handler 读 `AppState.stats` 返回 Json
- [x] 1.3 `app/src/state.rs`:`AppState` 加 `stats: Arc<RwLock<StatsSnapshot>>`,`new()` 初始化为全零快照
- [x] 1.4 采样任务(放 `routes/stats.rs` 内 `spawn_stats_sampler(state)`):2s 循环读
  - `/sys/fs/cgroup/cpu.stat` `usage_usec` 增量 → pct(首次 0;`cpu.max` 有效配额或 `available_parallelism`)
  - `/sys/fs/cgroup/memory.current` − `memory.stat inactive_file`;`memory.max`==max → None
  - `statvfs("/home/gem")` → used/total
  - 任一读取失败:`tracing::warn!` 并保留上次值
- [x] 1.5 `app/src/routes/mod.rs` + `main.rs`:注册模块、`.route("/api/stats", get(stats))`、启动时 spawn 采样任务
- 验证:`cd app && cargo check`;"deploy 后 `curl -u admin:admin localhost:8080/api/stats` 返回合理 JSON;`docker stats` 内存对拍差值 < 5%"→ 先本地 cargo check,容器级验证并入第 3 步

## 2. 前端(web/)

- [x] 2.1 `web/src/types.ts`:`StatsSnapshot` 接口(`memTotalBytes?: number`,镜像 design §2 契约,注释指向后端 owner)
- [x] 2.2 `web/src/useStats.ts`(新):3s 轮询 + AbortController + `online` 状态;导出 `useStats()`
- [x] 2.3 `web/src/App.tsx`:
  - 调 `useStats()`,manifest 首载结果播种 `online` 初值;`online`/`stats` 经 props 下发 Statusbar
  - GL effect:保存(stateChanged → 500ms 防抖 → `aio.layout` 存 minified saveLayout)、恢复(优先 saved config,catch 落回默认)、seqRef 重同步 helper
  - 重置回调:`removeItem("aio.layout")` + `location.reload()`,经 props 下发
- [x] 2.4 `web/src/icons.tsx`:补 copy / resetLayout 两个 symbol
- [x] 2.5 `web/src/Statusbar.tsx`:按 design §6 重排;dot 双态;stats seg(无数据整段隐藏);host 变复制按钮(clipboard API + execCommand 降级 + 1.5s `copied` 反馈);重置按钮
- [x] 2.6 `web/src/i18n.ts`:`statusOffline` / `copied` / `resetLayout` / `statsTip` 双语
- [x] 2.7 `web/src/styles.css`:`.dot.down`(红/灰)、stats seg 样式、复制反馈样式(窄屏沿用现有 ellipsis 溢出)
- 验证:`cd web && npm run build`(tsc 门)

## 3. 集成与冒烟

- [x] 3.1 `make up` 重建 app 镜像并起服务(`compose up --build` 对运行中服务的重建陷阱:必要时 `docker compose up -d --force-recreate app`)
- [x] 3.2 手动 AC 走查:AC1(分屏→刷新恢复→重置)、AC2(stats 读数 vs `docker stats`)、AC3(停 app 容器看 dot 变红/资源区隐藏)、AC4(复制,含局域网 IP 访问的非 secure context)、AC5(切换语言)
- [x] 3.3 `web/smoke-test.cjs` 扩展:页脚出现 CPU/MEM/DISK 文本;开第二个 tab → reload → 期待两个 tab(布局恢复);现有 6 断言全绿
- [x] 3.4 popout 回归:拖 tab 出窗 → dock back 正常(布局保存不破坏 SUB_WINDOW 路径)

## 4. 收尾

- [x] 4.1 3.2/3.3 发现的问题修复后复跑
- [x] 4.2 `trellis-update-spec`:布局持久化的 popout 已知局限、cgroup v2 读数口径(current−inactive_file)、clipboard 非 secure context 降级 → frontend/backend spec

## 风险文件与回滚点

- `web/src/App.tsx`(GL effect 一体改动):回滚点 = 2.3 完成后立即手动冒烟 popout + 默认布局
- `app/src/state.rs`: AppState 字段变更牵动所有 handler 构造处,`cargo check` 全量兜底
- 整体回滚 = revert 单 commit;localStorage `aio.layout` 残留无害(旧前端不读)

## task.py start 前检查

- [x] prd.md 收敛完成(无未决 Open Questions)
- [x] implement.jsonl / check.jsonl 已填真实 spec 条目
- [x] 用户已审阅三件套
