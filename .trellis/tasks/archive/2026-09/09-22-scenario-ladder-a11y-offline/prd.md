# L3 阶梯分组 + 勾选框可见性修复 + 离线环境配置验证

## Goal

承接 `09-22-scenario-items-layered-mise`（scenario 粒度重构）落地后的三个遗留问题，
owner 实机使用后提出：

1. **L3 语言层未按「阶梯」排列** —— 现在 rust/go/uv/ruff/c23 五个平铺在一坨，
   用户看不出「哪些是 mise 托管的、哪些是系统路径的」。要求按安装方式分阶呈现。
2. **前端待选方框看不见** —— 场景勾选框在页面上不可见，只能凭位置盲点。
3. **离线可用性未验证** —— 粒度重构后环境配置（场景选择）在离线场景下能否成立，
   需要完整实测（原父任务 AC10 未验）。

## Background

### 问题 2 根因（已定位，2026-09-22）

主题重构提交 `893baa6`（09-20-theme-schemes，已合入 main PR #29）在改写
`mgr-web/src/styles.css` 的 `:root` 时，**删除**了这一行：

```
-  --elev-ring: 0 0 0 1px var(--border);
```

但全仓仍保留 **24 处 `var(--elev-ring)` 引用**（`.check` / `.rows` / `.input` /
`.btn-secondary` / `.card` / `.dialog` / `.menu` / `.radio` 等）。CSS 自定义属性
未定义时，`box-shadow: var(--elev-ring)` 整条声明在 computed-value 阶段失效，
最终值为初始值 `none`。

对勾选框是致命的：`.check` 的样式是
`background: var(--surface); box-shadow: var(--elev-ring)`，
而它的父容器 `.rows` 同样是 `background: var(--surface); box-shadow: var(--elev-ring)`
—— 两者背景完全相同，**唯一的区分手段就是这个 1px 环**。环消失后，未勾选的
方框与容器背景融为一体，完全不可见。

这是**全站性**回归，不止场景选择器：所有卡片、输入框、次级按钮、对话框、菜单
都失去了边框环。

### 问题 1 现状

`config/src/scenario.rs` 只有 `category`（L1 os / L2 shell / L3 lang / L4 app）
一层分组。L3 下 5 个条目平铺：`c23`(apt 系统路径) / `go`(mise) / `ruff`(mise) /
`rust`(mise) / `uv`(mise)。安装方式只存在于 `description` 的自然语言里
（"mise aqua 后端,装 /opt/mise/installs" vs "apt 装系统路径"），**不是结构化数据**，
UI 无从据此分组。

父任务 PRD 的 R3 早已把 L3 定义为「两派：派 A mise 管理 / 派 B 系统管理」，
但这一语义从未落到 UI 上。

## Requirements

### R1 — L3 按安装方式分「阶梯」

- R1.1 为 scenario 引入**结构化的安装方式**元数据（而非从 description 猜），
  作为分阶依据。
- R1.2 L3 下按安装方式渲染**子标题（阶梯）**，每阶内为该方式的条目。
- R1.3 **阶梯只在层内存在多种安装方式时渲染**；单一方式的层（L2 shell 全 mise、
  L4 app 全 mise/npm）保持现状不加噪。
- R1.4 TUI（`config/src/tui.rs`）与 mgr-web（`EnvPicker.tsx`）两个面必须呈现
  一致的阶梯结构（cross-layer 一致性）。

### R2 — 修复勾选框与边框可见性

- R2.1 恢复 `--elev-ring` 定义，使 24 处引用重新生效。
- R2.2 **不得**只修 `.check` 一个点 —— 必须做全量审计：扫描所有
  `var(--token)` 引用与已定义 token 集合的差集，修复全部悬空引用。
- R2.3 修复需在**全部 8 套主题**下成立（token 取 `--border` 这类每主题值，
  定义放 `:root` 即可随主题变化）。

### R3 — 离线环境配置完整验证

- R3.1 验证 `make save` / `make load` 端到端机制不被粒度重构破坏。
- R3.2 验证**当前 `.aio/enabled.toml` 选出的全部场景**在无网络条件下功能可用
  （即「环境配置离线可解」）。
- R3.3 明确并文档化**边界**：运行期 `mise use -g <新工具>` 在离线机的行为
  （是否需要网络、卷缓存能否兜底）。

## Acceptance Criteria

- [x] AC1 — `--elev-ring` 恢复定义；全仓 `var(--*)` 悬空引用审计结果为 0
      —— 审计脚本比对 49 个引用 / 82 个定义，差集恰为 `--elev-ring` 一条；
      恢复后差集为 **0**
- [x] AC2 — mgr-web 构建产物中 `.check` 在未勾选态有可见边界
      —— 真实 chromium 渲染实测：**8/8 主题**下勾选框填充色与容器**完全相同**
      （坐实"环是唯一识别手段"），环对比度最差 **3.08**（vantablack），
      达 WCAG 1.4.11 的 3.0 下限；截图见 research/
- [x] AC3 — L3 在 mgr-web 上渲染为两个阶梯，L2/L4 不出现子标题
      —— 驱动**真实 SPA**（真 React + 真 API + 真 CSS）实测：
      L3 两个阶梯 `mise 托管`[Go/ruff/Rust/uv] + `系统路径·apt`[C23]；
      L2（10 项）与 L4（3 项）子标题数均为 **0**
- [x] AC4 — TUI 同样渲染 L3 阶梯，`cargo test` 全绿
      —— pty 捕获真实 TUI 帧：L3 出现 `└ mise 托管` / `└ 系统路径·apt` 两个阶梯，
      L2 无阶梯；`cargo test` config **27** + mgr **98** 全绿
- [x] AC5 — `make save` 产出 bundle，`make load` 后可 `make up NOBUILD=1` 起来
      —— bundle **2.5G**（22.7s）；含 5 镜像 / 143 blob **零缺失**；
      `make load` 五个镜像全部 Loaded；`make up NOBUILD=1` 栈正常运行
- [x] AC6 — 离线（`--network none`）下 enabled.toml 中每个场景的代表性命令全部可用
      —— **login 35/35 + 非 login 35/35，0 失败**；`pi-web` 首轮报 HANG 系探针
      命令错（它是 Next.js 服务无 `--version`），改探活后离线 HTTP 200
- [x] AC7 — 离线机运行期 `mise use -g` 的行为有明确实测结论并写入文档
      —— 新工具离线**快速显式失败**（DNS，不挂起）；**已烘焙**工具离线
      `mise use -g` **成功**（回落本地）；结论写入
      `docs/offline-install-guide.md` §3.6

## Out of Scope

- 重做主题体系 / 新增主题
- 已归档父任务 `09-22-scenario-items-layered-mise` 的 AC6/AC9 独立复验
  （本任务的 AC6 会顺带覆盖其环境配置部分）

## Notes

- 关键文件：`config/src/scenario.rs`、`config/src/tui.rs`、
  `mgr-web/src/pages/EnvPicker.tsx`、`mgr-web/src/i18n.ts`、
  `mgr-web/src/styles.css`、`scenarios/*/scenario.toml`、`Makefile`、`docs/`
