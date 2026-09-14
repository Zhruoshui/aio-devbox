# mgr-web 六项体验升级（父任务）

## Goal

用户六项改进诉求的统筹任务：服务可选、场景层级、镜像管理、模型配置重构、
用量图表、侧栏折叠。本任务持有源需求、任务地图、跨子任务验收与最终集成
审查；具体实现由五个子任务承载。

## 源需求（用户原话摘要，2026-09-10）

1. code-server/vnc/pi-web 是可选服务，创建沙箱时可供选择
2. 新建沙箱场景选择按用户层级（L1os/L2shell/L3lang/L4app）排列
3. 镜像页面添加镜像说明，且能够管理镜像
4. 模型配置重构：统一界面配置好模型后，按 agent（pi/claude code/codex/
   opencode）点击选择对哪些沙箱生效
5. 用量统计补图：合计图能看到不同沙箱耗费；单沙箱统计更细、图也更多
6. 管理器左侧折叠只露图标；沙箱内选项按钮栏收纳，最大化工作区

## 决策记录（访谈定案 D1-D7）

- **D1 服务可选 = 控制"装"**：创建向导四服务开关（code-server/vnc/pi/
  pi-web，默认全开）。关闭 → compose 不含该服务且镜像不构建；pi 开关 =
  pi 场景、pi-web 开关 = pi-web 场景的 UI 升格。已建沙箱不可改。连带改
  composegen + jobs（构建条件化）+ create/edit API + envhash（组合哈希
  含服务开关）。
- **D2 场景层级**：EnvPicker 按 category 分四节「系统 (L1) / Shell (L2) /
  语言 (L3) / 应用 (L4)」双语标题；场景卡片补 description 展示（API 已
  返回）。服务四项只出现在服务开关区，不出现在场景区；开 pi-web 强制
  开 pi + vnc（依赖校验在 mgr 端）。
- **D3 镜像页**：说明 = images 表新增组合清单列（构建时写入场景+版本+
  服务开关）+ 实时体积（docker inspect）；管理 = refcount=0 单删（禁用态
  显示原因，连带 base/app/cs 镜像组）+ 一键清理未引用 + 构建缓存清理。
  不做重建按钮。推翻旧设计 §3.6 "不做删除"。
- **D4 模型配置重构**（cc-switch 心智）：(a) 全局供应商库保持；(b) profile
  × agent 指派，四 agent 各自指向供应商库一项，卡片式一键切换；(c) 沙箱
  指派 = profile + agent 子集，mgr_sync 带集合只渲染勾选 agent。不做沙箱内
  每 agent 各挑 profile。渲染器复用 app/src/routes/models/render/ 四件套。
- **D5 用量图表**：合计视图加「分沙箱」横向条形图（mgr 端拼接，点条跳
  沙箱视图）；单沙箱加「按天趋势」（近 14 天 token/成本双系列，app
  usage.rs 增 byDay）；明细表加日期列 + 按日筛选。opencode 解析不做。
- **D6 侧栏折叠**：管理器侧栏折叠露图标（~48px，状态 localStorage）；
  SandboxTree 折成竖向首字母图标条，hover flyout 展服务按钮组；
  golden-layout tab 条不动。
- **D7 拆分与排序**：S1→S2→S3→S4→S5（见任务地图）。

## 关键代码事实（勘察锚点）

- composegen.rs:115-138 sidecar 段；docker.rs:115 UP_PROFILES 写死 vnc；
  jobs.rs:133 镜像无条件构建（app/cs/vnc）
- EnvPicker.tsx（110 行）平铺未分组；category_rank 在 config/src/
  scenario.rs:76（os=0<shell<lang<app），后端 scenarios API 已带
  category/always_on/description
- ImagesPage.tsx（96 行）无删除无说明；db.rs:49 images 表无组合列
- 模型：D8 profile 指派 PUT /api/sandboxes/:name/model_profile（routes.rs:47）；
  mgr_sync.rs 60s 拉取；aio-models store.rs AgentsConfig 本就 per-agent；
  render/{pi,claude,codex,opencode}.rs 已存在
- 用量：UsagePage.tsx 已有 TokenBars/CostDonut/chip 选择器；mgr/src/usage.rs
  不跨沙箱聚合；app usage.rs 解析 pi/claude/codex 三家（无 opencode），
  时间戳处理基础设施已有（days_from_civil:917）
- 侧栏：App.tsx:115 aside.sidebar；WorkspacePage 左侧 SandboxTree
  （239 行）+ golden-layout

## 任务地图（子任务，见各目录 prd.md）

| 子任务 | 承载 | 依赖 |
|---|---|---|
| S1 mgr-create-services | D1+D2 创建向导 + 服务开关数据模型 | 无（最先：API/envhash 定型后 S3 不返工） |
| S2 mgr-models-agent-assign | D4 模型三层 | 无硬依赖，建议在 S1 后 |
| S3 mgr-images-manage | D3 镜像说明+管理 | 依赖 S1 的组合清单字段（服务开关进组合） |
| S4 mgr-usage-charts | D5 图表 | 无硬依赖 |
| S5 mgr-sidebar-collapse | D6 折叠 | 无（可随时插队） |

## 跨子任务验收

- [ ] 六项决策全部落地且互不回归：服务开关组合 → envhash → 镜像组合清单
      → 镜像页展示（S1→S3 链路数据贯通）
- [ ] cargo test -p aio-mgr -p aio-app 与 mgr-web tsc/build 全绿
- [ ] make mgr-up 重建后全部页面可用；创建/编辑/删除沙箱全流程实测
- [ ] 旧沙箱（无服务开关数据）向后兼容不炸

## Out of Scope

- 沙箱内每 agent 各挑 profile（交叉混搭）
- opencode 用量解析（格式未勘察，单独任务）
- 镜像重建按钮；golden-layout tab 条改动
- code-server/vnc 共享单实例（09-09 头脑风暴 D2/D3 已否决，维持现状）
