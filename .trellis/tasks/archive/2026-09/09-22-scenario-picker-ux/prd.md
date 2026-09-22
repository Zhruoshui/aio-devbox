# picker UX: L2/L3 折叠与全选 + 镜像组合说明点击查看详情 + 构建日志增强

## Goal

三块 mgr-web/TUI 交互增强（背景：scenario 粒度重构后 L2 shell 有 10 项、L3 lang 5 项、
L4 app 3 项，选择器变长）：

1. **L2/L3 可选项太多** → 支持层折叠 + 层内全选/全不选（mgr-web EnvPicker + config TUI）
2. **镜像列表的「组合说明」** → 从 title 悬浮提示升级为可点击查看具体配置详情
   （scenario 清单 + 各自版本 + 服务开关）
3. **构建日志** → 构建镜像时展示较完整日志便于调试

## Requirements

### R1 — EnvPicker（mgr-web）层折叠 + 全选

- R1.1 每个 layer（L1-L4）的标题行可点击折叠/展开，默认**展开**。
- R1.2 层标题旁提供「全选/全不选」：仅作用于该层内**非 always_on** 的可选项
  （always_on 行保持锁定勾选，不参与批量操作）。
- R1.3 折叠不丢失选择状态（纯 UI 状态，不影响 env 数据）。
- R1.4 已选计数显示在层标题上（如 `L2 shell · 3/10`），折叠时也可见。

### R2 — ~~config TUI 层折叠 + 全选~~（已裁掉）

用户决策（2026-09-22 规划期）：**config TUI 不动**。TUI 是全仓库预设编辑器、使用频率
低，投入产出比不足；交互增强集中在 mgr-web 创建向导/编辑页。TUI 若将来需要可单开任务。

### R3 — 镜像列表组合说明点击查看详情

- R3.1 ImagesPage 的 combo 列从 `title` 悬浮升级为可点击：展开一个详情区
  （复用现有 build-log 展开行的交互模式），展示**结构化**内容：
  - 每个勾选的 scenario（名称 + id + 选中版本；未显式选版本时标注默认）
  - always_on 项及其版本（标注「必装」）
  - 服务开关（code-server / vnc / pi / pi-web）
- R3.2 数据从后端取：`/api/images` 响应新增 `env` 字段（该镜像构建时的
  SandboxEnv 规范化 JSON + services 布尔组），**不回填解析旧 combo 字符串**——
  历史行（无 env 数据）详情区回退为仅展示 build log。
- R3.3 实现：db images 表新增 `env_json` / `services_json` 列（pragma-probe 迁移，
  与 combo 列同款模式）；create job 成功 upsert 时写入；types.ts `Image` 镜像新增字段。

### R4 — 构建日志增强

- R4.1 **实时流式**：mgr 后端 create/recreate job 在构建阶段逐行转发 docker build
  输出到 job log（当前是构建结束一次性追加），JobView 轮询下即可呈现滚动日志。
- R4.2 **镜像页留全量**：images 表 `build_log` 从 8KB tail 提升到 **256KB** tail
  （persona-scale 个人使用，256KB 足以覆盖绝大多数失败现场），JobView 运行中
  仍保持 8KB。
- R4.3 docker build 失败时 stderr 一并进 job log（当前 run_capture 只带 stderr
  tail 进 anyhow error，job error 字段可见但 log 里无构建输出）。

## Acceptance Criteria

- [x] AC1 — EnvPicker：折叠 L2 后提交创建，env.scenarios 与折叠前完全一致；
  层全选后再取消单个项，提交结果精确等于手工勾选
  —— 机制验证：折叠/全选是纯 UI 状态，经 API 创建实测 env_json 逐字节正确；
  `setLayerAll` 为并集/差集语义（全选不重复添加、全不选只删本层 id），单测等价。
  **Owner 目视待办**（tsc 已验，交互帧未截）。
- [x] AC2 — EnvPicker 层全选：L2 一次全选 10 项后，env.scenarios 恰含 10 个 shell id
  且无其他层污染；always_on 项不受影响
  —— `setLayerAll` 仅遍历 `!always_on` 项；always_on 在 API 层被拒于 env.scenarios
  （契约双重保证）。
- [x] AC3 — ImagesPage：点击 combo 展开详情，能看到 scenario 名称+版本与服务开关；
  无 env 数据的旧行展示回退 —— API 实测：`uitest1` 行
  `env={"scenarios":["fzf"],"versions":{node,python}}` + `services={code_server:false,vnc:false}`；
  5 条 pre-R3 行 `env:null/services:null`（前端走 imgDetailEmpty 回退）。
  **Owner 目视待办**（渲染未浏览器截帧）。
- [x] AC4 — 失败构建：JobView 过程见滚动构建行（实测构建中 job log 4.6KB 持续增长，
  BuildKit 行逐条到达）；失败行 `1ae1b838…` build_log 2KB 完整含
  `ERROR: failed to build … exit code: 42`；成功行 6165B 远超旧 8KB 截断场景。
  附带增强：失败构建现在留 images 表行（原无任何痕迹）。
- [x] AC5 — `cargo test -p aio-mgr`（99 绿，含 db 迁移/upsert 新测试）、
  `cargo clippy -p aio-mgr` 无警告、mgr-web `npm run build`（tsc 门 + vite）通过。

## Out of Scope

- combo 字符串本身的格式不变（继续用于列表列展示与兼容）
- JobView 从轮询改为 SSE/WebSocket（流式由 1.5s 轮询呈现即可）
- 旧镜像行的 env 详情回填（历史数据不回溯）
- mgr-web 构建期日志的浏览器自动滚动优化（已有 scrollTop 逻辑）

## Decisions（2026-09-22 规划期用户确认）

- D-a — 范围：**mgr-web + mgr 后端，不动 config TUI**。
- D-b — 镜像详情数据源：**images 表新增结构化 env_json/services_json 列**，不回填历史。
- D-c — 日志增强：**构建期流式转发 + images build_log 提到 256KB**。

## Open Questions

- Q1 — 详情区交互形态：复用「行内展开」（与 build log 同一行展开位）还是独立
  对话框？（倾向行内展开，与现有模式一致）
- Q3 — 构建流式日志的缓冲：逐行转发 vs 小批量 flush（实现期定，锁竞争可忽略则逐行）。
