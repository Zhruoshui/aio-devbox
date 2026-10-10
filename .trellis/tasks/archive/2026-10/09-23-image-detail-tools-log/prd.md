# image-detail-dialog custom-tools-editor build-log-full

## Goal

优化 mgr-web 的镜像详情交互、补齐用户自定义工具的 WebUI 编辑入口、让沙箱构建日志可完整回看。三件事互相独立，但都是 mgr-web + mgr 的配套改动，放在一个任务里一次交付。

## Background

- 镜像列表（`ImagesPage`）的组合说明当前在表格行内展开，把表格挤变形，信息密度也不够清晰。
- 用户自定义工具（`scenarios/user-tools/fragment.Dockerfile`）目前只能手切到宿主机/code-server 改文件，WebUI 没有编辑入口。它已经是独立 `RUN` 层（BuildKit 单独缓存），"写错单独重编"在缓存层面成立，缺的是 UI。
- 构建日志后端只存尾部：job 内存 8KB（`LOG_TAIL`）、images 表 256KB（`BUILD_LOG_TAIL`），完整输出从不持久化。用户只能看尾巴，看不到从头到底。

## Requirements

### R1 镜像组合说明 → 弹窗式详情

- 列表页"组合说明"列保留一行短摘要，点击改为打开居中弹窗（复用 `components/Dialogs.tsx` 的模式），不再行内展开。
- 弹窗内容分块展示：构建时选择的场景及各自版本、必装集合、搭载的服务开关（code-server / vnc / pi / pi-web）、镜像大小、构建时间、env_hash。
- 旧记录（`env`/`services` 为 null）在弹窗里给明确提示，不显示空区块。
- 表格行内展开槽只留给"构建日志"一个用途，不再与 combo 抢同一槽位。

### R2 用户自定义工具 WebUI 编辑（B1+B2 混合）

- 新增 mgr API：读取 / 保存 `scenarios/user-tools/fragment.Dockerfile` 的内容（纯文本读写，原子写入）。
- 新增 mgr-web 页面/入口：两种编辑模式共存——
  - **结构化表单**：一行一个工具，字段包含名称、安装方式（apt / tarball / script）、版本或 URL、安装到 `/usr/local/bin` 或 `/opt/<tool>`；表单生成对应 Dockerfile 片段。
  - **高级：直接编辑 Dockerfile**：带等宽字体的 textarea，用户自由写任意片段。
- 结构化表单生成的内容要能无损回落到"直接编辑 Dockerfile"，两者看到的是同一份文件。
- 表单模式要阻止已知会出错的写法：写 `/root`、不写版本双通道自检等，给出明确报错。
- 保存后提示用户触发重建（`make gen` + build）的路径；本任务**不实现** mgr 主动触发重建（仍走现有 Makefile / 手动流程）。
- 保留 `scenarios/user-tools/scenario.toml` 里已有的三条硬规则说明，在 UI 里以提示形式露出。

### R3 构建日志完整可看（C1 落盘 + 调大）

- mgr 把 job 的**完整**构建输出落盘到文件（按 job id 命名），数据库 job 行加 `log_path` 列，不再依赖内存截断。
- 内存中的 `LOG_TAIL` 从 8KB 提升到 64KB（轮询期间前端的即时可见窗口更大）。
- 落盘的 `BUILD_LOG_TAIL` 从 256KB 提升到 4MB（images 表 `build_log` 列保留尾部作为索引/快速预览）。
- 新增 mgr API：按 job id 获取完整日志，支持 `Range` / `offset` / `tail` 参数，前端可分段加载。
- `JobView` 改为"流式跟随 + 可向上翻"：默认滚到底跟随新增；用户向上滚动时暂停跟随，滚回底部恢复；提供"跳到顶部"按钮从头看。
- `ImagesPage` 的"构建日志"改为加载完整日志（按 job 或按 image 关联的日志文件）。
- 修复顺带发现的 bug：`jobs.rs:614-616` 的 8KB 截断未做 char 边界对齐，多字节输出跨边界会 panic，照抄同文件 `tail()` 的对齐逻辑。

## 三条修正（review 后调整，本任务的最终交付形态）

用户 review 后对 R1 / R2 提出三点修正，覆盖原 R1/R2 的部分设计：

### 修正 1 — 组合说明列只留短摘要（不再展示长说明）

镜像列表"组合说明"列只保留一行短摘要（截断 + 省略号），点开已实现的详情弹窗看完整信息，不再行内展开长说明。

### 修正 2 — 保留自定义工具"自由配置"能力，Dockerfile 手改用简易文本编辑器承载

- `mgr` 外部管理的部分（宿主机 / code-server 直接改 `fragment.Dockerfile`）保持现状，可自由配置。
- WebUI 的"直接编辑 Dockerfile"入口改为一个**简易文本编辑器**（等宽 textarea + Tab 缩进 + 状态行），而不是现在这个"双 Tab + 表单解析"的复杂形态。
- **结构化表单模式移除**：不再有"表单 ⇄ 文件"的双向解析 / 回落逻辑。解析复杂度不值得——文件已经是事实标准。
- 保留三条硬规则提示 + 保存后的重建路径提示；保存仍走原子写 + 静态校验（/root 硬规则）。

### 修正 3 — 用户自定义工具提升为独立的 L5 层，创建沙箱时可自由勾选

- 自定义工具从 L2 (`category = "shell"`) 提升为 **L5 (`category = "service"`）**。
- 每个在外部管理创建的自定义工具成为一个**独立的 scenario**（`scenarios/<tool>/`），其 `fragment.Dockerfile` 就是该工具的安装片段。
- 创建沙箱的 EnvPicker 增加 L5 区，列出所有自定义工具，可像其他场景一样自由勾选；勾选结果（工具 id 集合）进入 env_hash，保证不同工具集合产生不同镜像。

## Non-Goals（修正后）

- 不实现 SSE / WebSocket 流式推日志（仍走 1.5s 轮询，只是把窗口放大 + 完整落盘）。
- 不在 mgr 里实现"触发镜像重建"的按钮，仍由用户走 Makefile。
- L1–L4 层的场景构成 / 分层模型不变。
- 不改按钮注册（`buttons.toml`）这条运行期自定义通道。
- L5 工具**不做版本选择**（WebUI 编辑器承载版本/URL 语义，env 里只有 id）。
- 移除 R2 的结构化表单生成 / 解析；WebUI 只剩"直接编辑"一条路径。

## Acceptance Criteria

- [x] **AC1（R1）**：镜像列表点击"组合说明"打开居中弹窗，展示场景+版本、必装、服务开关、大小、构建时间、env_hash；旧记录显示提示而非空区块；行内展开只剩"构建日志"。
- [x] **AC2（R1）**：弹窗可用 Esc / 点遮罩 / 关闭按钮关闭，样式与 `ConfirmDialog` 一致。
- [ ] **AC3（修正1）**：镜像列表组合说明列只显示一行短摘要（长说明不再出现）；点开弹窗看完整组合。
- [ ] **AC4（修正2）**：自定义工具页为单一"直接编辑"模式（简易文本编辑器：等宽 + Tab 缩进 + 状态行）；不再有表单/解析回落；保存原子写 + /root 校验 + 重建路径提示。
- [ ] **AC5（修正3）**：`scenarios/user-tools/` 不再是一个 scenario；每个自定义工具是 `scenarios/<tool>/` 独立 scenario（L5）。`GET /api/scenarios` 返回这些 L5 条目，EnvPicker 渲染独立 L5 区可勾选。
- [ ] **AC6（修正3）**：L5 工具勾选集合进入 env_hash——勾选不同 L5 工具集合产生不同 env_hash / 镜像 tag；勾掉某个工具后重建该沙箱，新镜像不含该工具。
- [ ] **AC7（修正3）**：`PUT /api/user-tools` 保存后，新工具的 scenario 目录 / `scenario.toml` / `fragment.Dockerfile` 立即出现在 `GET /api/scenarios` 的 L5 分组里（无需重启 mgr）。
- [ ] **AC8（R3）**：一次完整构建后，磁盘上存在包含全部输出的日志文件（行数与构建实际输出一致，无截断）。
- [ ] **AC9（R3）**：`JobView` 默认跟随到底部；向上滚动后停止跟随，回到底部恢复；点"跳到顶部"能加载从头开始的内容。
- [ ] **AC10（R3）**：`ImagesPage` 的"构建日志"能加载超过 256KB 的完整构建输出。
- [ ] **AC11（R3）**：含中文/emoji 的构建输出跨 8KB/64KB 边界时不再 panic，截断处字符完整。
- [ ] **AC12**：`cargo test`（mgr + config）与 `npm run build`（mgr-web）均通过。

## Notes

- 三块互相独立，可分别验证，但共享 `ImagesPage` / `JobView` 的样式与 `Dialogs.tsx` 的弹窗模式，放在同一任务一次交付更经济。
- R2 的"表单生成 Dockerfile"采用**单向生成 + 回落**：表单始终从当前文件解析出工具列表；文件一旦被手改成表单认不出的形状，表单模式提示"已切换到直接编辑模式"，避免双向同步的复杂度。
