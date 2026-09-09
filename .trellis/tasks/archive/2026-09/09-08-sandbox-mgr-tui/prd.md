# 多沙箱 Web 管理界面（sandbox-mgr）

## Goal

把现有单沙箱工作台扩展为**多沙箱统管**：新增独立 mgr 栈（mgr-api + mgr-web），
作为唯一管理入口，管理 N 个沙箱实例（每沙箱 = 独立 compose project），
覆盖生命周期、环境配置、资源配置、模型配置上收与统一入口路由。
现有单沙箱栈代码不动，退化为「被管理的第 1 个沙箱」。

## Background

- 现有栈：gateway(caddy:8080, basicauth) + app(axum:8088, 发布 pi-web 30141) +
  code-server/vnc 侧车（`network_mode: service:app` 共享 netns, profile 门控）；
  单 compose project，单 `workspace` 卷。按钮/面板来自 `services.toml`
  （编译进 app）+ `/root/.aio/buttons.toml`（运行时注册）。
- 场景装配是 build-time 机制：`aio-config`（`config/` crate）`tui` 写
  `.aio/enabled.toml`（场景 + [[versions]] 版本），`gen` 拼 `Dockerfile.base`，
  `make build-base` 构建镜像。
- 模型配置在沙箱 app 内 `/api/models/*`（canonical store + render 派生到
  pi/opencode/claude/codex 各自配置文件 + usage 聚合），按沙箱自持。
- pi-web 因 Next.js 根绝对资源路径必须独占 origin（子路径代理不可行）；
  code-server/vnc 经每沙箱 gateway 子路径代理（原样保留）。

## 已确认决策（D1–D12）

- **D1 形态**：新增独立 mgr 栈（mgr-api + mgr-web）；mgr 与沙箱生命周期分层，
  沙箱不可承载 mgr。现有栈不动。
- **D2 控制通道**：DooD + compose CLI。mgr 为每沙箱生成 compose 文件，
  `docker compose -p sbx-<id>` 独立 project 起停；不用 Docker API 重实现
  compose 语义；生成物可 diff、可宿主机手工接管。
- **D3 运行形态**：mgr 本地裸跑（单二进制直连宿主 docker）与容器化
  （挂 `/var/run/docker.sock`）双支持，不得硬依赖容器环境。
- **D4 环境配置→镜像**：保留 build-time。mgr-web 场景/版本勾选 UI 替代
  `make config` TUI；mgr-api per-sandbox 持久化 env 配置，复用 `aio-config gen`
  逻辑拼 Dockerfile.base，tag `sandbox-base-<env-hash>`；相同配置共享镜像 tag。
  `make config` TUI 保留作 CLI 兜底。
- **D5 code-server**：不动（per-sandbox 侧车，共享镜像 tag，profile 门控）。
  「集中 code-server + Remote-SSH」不可行（Remote-SSH 微软专有，Open VSX 无）；
  桌面 VSCode 直连如需要后续加 sshd 场景。
- **D6 模型配置上收**：mgr 唯一真相源（复用 store.rs schema）；沙箱 app 新增
  拉取端点（启动时 + 定时）写本地 canonical store 再走现有 render 链路；
  沙箱内写接口降级只读，配置页 UI 迁 mgr-web，沙箱内留只读视图；
  mgr→沙箱内网明文；离线拉不到用本地缓存。usage 本地聚合不变，mgr 汇总展示。
- **D7 资源配置**：仅 CPU/内存（compose `deploy.resources.limits`，
  per-sandbox 生成时填入）。磁盘配额不做。
- **D8 入口路由**：mgr 栈内 caddy 总网关，按 Host 头把
  `<sandbox>.mgr.localhost` → 该沙箱 gateway、
  `<sandbox>-piweb.mgr.localhost` → 该沙箱 pi-web（同端口多 origin，解决
  多 pi-web 端口问题）。沙箱内部子路径路由不动。LAN 端口段兜底后置。
- **D9 认证**：全面移除密码/认证（mgr 总网关与各沙箱 gateway 均无 basicauth；
  hash 机制随 mgr 生成的 compose 移除，存量栈同样去认证）。
- **D10 UI 形态**：两层入口。mgr-web 为独立管理 SPA（沙箱列表/创建向导/
  环境配置/模型配置/usage 汇总）；「进入沙箱」= 新标签页跳转
  `<sandbox>.mgr.localhost` 的现有工作台 SPA（零改动）。不做 iframe 嵌入。
- **D11 离线分发**：本期不做多沙箱化，`make save/load` 维持单沙箱现状。
- **D12 技术栈/状态**：mgr-api = Rust axum 新 crate（repo 平级，`aio-config`
  经 workspace 成员依赖复用）；mgr-web = React SPA（复用 web/ 构建模式与
  设计 token）；状态 = SQLite（rusqlite）单库于 mgr 数据目录；per-sandbox
  compose 文件落盘 `instances/sbx-<id>/compose.yml`。

## Requirements

- **R1 mgr 栈**：新 crate（axum）+ mgr-web（React SPA）+ 总网关（caddy）；
  本地裸跑与容器化双形态；SQLite 状态库；无认证。
- **R2 沙箱生命周期**：创建 = env 配置持久化 → gen 拼装 → build env-hash
  镜像（同配置复用，跳过构建）→ 生成 compose → `compose -p sbx-<id> up -d`；
  停止/启动/重启/删除（删除含卷，UI 需确认交互）。创建/构建为长任务，
  UI 有进度与失败原因展示。
- **R3 环境配置 Web 化**：场景 + 版本选择 UI（等价 `aio-config tui` 能力，
  含 always_on 场景不可取消的语义）；per-sandbox 持久化；镜像 tag、构建状态、
  引用沙箱数可见。
- **R4 资源配置**：per-sandbox CPU 核数 / 内存上限，写入生成 compose 的
  `deploy.resources.limits`。
- **R5 模型配置上收**：mgr 端 canonical 配置 CRUD（含 discover/test 等既有
  能力迁移）+ 下发（沙箱拉取端点）；沙箱内 `/api/models` 写接口禁用/只读、
  模型配置页只读；usage 汇总展示（各沙箱本地聚合 + mgr 聚合视图）。
- **R6 统一入口**：总网关按 D8 子域名分流；沙箱工作台、code-server、vnc、
  pi-web、web terminal 均经子域名访问；无认证。
- **R7 存量纳管**：现有单沙箱栈可导入 mgr 登记（不重建即被管理）。
- **R8 mgr-web 页面**：沙箱列表（状态/资源/镜像/入口链接）、创建向导
  （env + 资源 + 名称）、环境配置编辑、模型配置页（自沙箱 UI 迁移）、
  usage 汇总、镜像列表（env-hash tag 与引用计数）。

## Acceptance Criteria

- [ ] A1 mgr 本地裸跑启动，mgr-web 可访问；容器化形态（挂 socket）同样可启动。
- [ ] A2 创建沙箱全流程：向导选场景/版本/CPU/内存 → 镜像构建 → compose up →
      列表出现且状态 running；构建失败时 UI 展示失败原因。
- [ ] A3 `<sandbox>.mgr.localhost` 打开该沙箱工作台：terminal / code-server /
      vnc / pi-web 面板可用，全程无密码提示。
- [ ] A4 `<sandbox>-piweb.mgr.localhost` 打开该沙箱 pi-web（多沙箱各自独立）。
- [ ] A5 相同 env 配置创建第二个沙箱：镜像不重建（tag 复用），两沙箱
      project/卷/数据互相隔离。
- [ ] A6 mgr 修改模型配置 → 沙箱内拉取生效（启动拉取或等待定时）；
      沙箱内模型配置页只读；usage 在 mgr 汇总可见。
- [ ] A7 停止/重启/删除沙箱生效；删除需确认且含卷清理；删除后对应
      子域名不可达。
- [ ] A8 `docker inspect` 验证 CPU/内存 limits 与配置一致。
- [ ] A9 存量单沙箱栈导入 mgr 后被正确管理（列表可见、可启停）。
- [ ] A10 mgr 生成的 compose 文件落盘且 `docker compose -p sbx-<id> ps`
      可在宿主机手工执行（D2 可接管性）。

## Out of Scope

- 磁盘配额（D7）；多用户/账号体系（D9）；mgr-web 内嵌沙箱工作台（D10）；
  离线多沙箱分发（D11）；code-server 架构变更（D5）；LAN 子域名解析与
  端口段兜底（D8 后置）；sshd 场景（D5 后置，需要时再做）。
