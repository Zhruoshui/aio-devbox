# design.md — Scenario 粒度重构

> 本文件是**父任务**的设计。两个子任务各有自己的 design.md 记录实现细节：
> - `.trellis/tasks/09-22-scenario-granularity-refactor/`（构建链路重构）
> - `.trellis/tasks/09-22-mise-runtime-volume-config/`（运行时卷化）

## 1. 架构总览

### 1.1 四层与「两派」

L1/L2/L3/L4 的 `category` 分层**已存在**（`config/src/scenario.rs` 的
`CATEGORY_ORDER` + `category_rank`），本次不新增层级，只细化每层内容。

L3 的「两派」是**并级两个 scenario**，不是互斥选项：

| 派 | 机制 | scenario | 装载路径 |
|---|---|---|---|
| A · mise 管理 | `scenario.toml` 的 `versions` + fragment 内 `mise use -g` | rust / go / uv / ruff | `/opt/mise`（shims 在 PATH） |
| B · 系统管理 | apt 装系统路径 | c23 | `/usr/bin`、`/usr/local/bin` |

用户可同时选中（rust 写库 + clang 写 C），二者路径不冲突、无构建期依赖。

### 1.2 重构后的 scenario catalog

```
L1 os      mise          [always_on, 新]  ← engine only，无工具
           node          [always_on]      ← 不变
           python        [always_on]      ← 不变
           fonts         [可选]           ← 不变
L2 shell   fzf ripgrep bat fd eza zoxide delta starship jq yq   ← 新增 10 个
L3 lang    rust go uv ruff                                       ← mise 派，新
           c23                                                   ← 系统派，不变
L4 app     opencode codex claude-code                            ← 新增 3 个
           pi            [留在服务区]      ← CLI 改由 mise 装
           pi-web        [留在服务区]      ← 不变
```

`scenarios/mise`（原五工具全家桶）与 `scenarios/shell-utils` **删除**（D3）。

## 2. 核心机制：为什么不需要新的 gen 能力

### 2.1 关键实测（本设计的立足点）

三个独立 RUN 层各跑 `mise use -g <tool>` 后，`/opt/mise/config.toml` 的 `[tools]`
段是三者**并集**，无覆盖：

```toml
[tools]
fd = "10.5.0"
jq = "1.8.2"
starship = "1.26.0"
```

原因：`mise use -g` 是**读改写**语义（读现有 config → 合并 `[tools]` → 写回），
对 Docker 顺序层天然幂等可组合。

**推论**：`gen` 保持现有的纯字符串拼接不变。**不需要**「横向聚合 + 占位符注入」，
也**不需要**依赖图。这否决了规划早期的复杂方案。

### 2.2 fragment 模板（三种）

**模板 1 — mise 工具（大多数）**

```dockerfile
# >>> scenario: fzf >>>
ARG FZF_VERSION=0.74.4
RUN mise use -g "fzf@${FZF_VERSION}" \
 && bash -lc 'command -v fzf' \
 && fzf --version
# <<< scenario: fzf <<<
```

**模板 2 — mise 工具，需 table 形式 spec（仅 rust）**

`mise use -g` 只能写 `tool = "ver"` 简单形式；rust 需要
`rust = { version = "...", profile = "default" }`，**必须直接写 config**：

```dockerfile
RUN printf 'rust = { version = "%s", profile = "default" }\n' "$RUST_VERSION" \
      >> /opt/mise/config.toml \
 && mise install rust \
 && mise exec -- rustup component add rust-analyzer \
 && bash -lc 'for t in rustc cargo clippy rustfmt rust-analyzer; do command -v $t || exit 1; done'
```

两个补偿缺一不可：
- `profile = "default"` — rustup 默认 profile 是 minimal，会丢 clippy/rustfmt
- `component add rust-analyzer` — rust-analyzer 不在任何 profile 里；**缺失时
  rustup 代理沿 PATH 撞上 mise shim，shim 再指回代理 → 无限递归**（PoC 实测）

**模板 3 — apt 包（c23）** — 保持现状，`apt-get install` + 软链 + 冒烟测试。

### 2.3 前置依赖：fragment 假定 engine 已就绪

每个 mise fragment 都假定 `/opt/mise` 与 `MISE_*` env 已由 L1 的 mise engine
scenario 建立。这**不是构建期依赖**（fragment 之间无依赖图），而是 **layer 排序保证**：
`gen` 的 `sort_by_layer` 把 `category = "os"` 排在 `category = "shell"/"lang"/"app"`
之前，engine 的 RUN 层必然先执行。

**风险**：若用户通过某种方式只选 L2/L3/L4 而不选 L1（当前不可能——L1 engine 是
always_on），fragment 会在缺 `mise` 命令时构建失败并给出清晰错误。这是可接受的
fail-fast。

## 3. 契约与兼容性

### 3.1 哪些契约**不变**（这是本次设计的最大优点）

| 模块 | 是否改动 | 说明 |
|---|---|---|
| `config/src/scenario.rs` | **不变** | `ScenarioMeta` 字段足够；只是目录变多 |
| `config/src/gen.rs` | **不变** | 纯拼接逻辑与 layer 排序照旧 |
| `config/src/manifest.rs` | **不变** | `Enabled{scenarios, versions}` 语义完全够用 |
| `config/src/tui.rs` | **不变** | 按 category 分组渲染，自动适配新目录 |
| `mgr/src/envhash.rs` | **不变** | `SandboxEnv` 契约不变；env_hash 自动跟随字节 |
| `mgr-web/types.ts` | **不变** | `Scenario` / `SandboxEnv` 类型不变 |

**唯一需要改的前端文件**是 `mgr-web/src/pages/EnvPicker.tsx` 的
`SERVICE_SCENARIOS` 常量——新增的 opencode/codex/claude-code 不在该列表中，
自动落入 L4 场景区（默认行为），**无需改动**。pi/pi-web 保持原样（D2）。

### 3.2 破坏性变更

**删除的 scenario id**：`mise`、`shell-utils`。

- 仓库 `.aio/enabled.toml` 引用它们 → 运行 `make config`（TUI）重选，或直接改文件
- 旧 sandbox `feiver` 的 `env_json` 引用它们 → `envhash.rs::to_manifest_checked`
  会对**编辑该 sandbox** 报 `unknown scenario`
- **处理策略（D3）**：不保留空壳。feiver 本来就要重建（场景变化 → env_hash 变化
  → 镜像 tag 变化），重建即可

### 3.3 env_hash 自动敏感

`env_hash = sha256(组装后的 Dockerfile.base 字节)`（`mgr/src/envhash.rs`）。
item 级选择变化 → fragment 内容变化 → 字节变化 → hash 变化。**无需额外实现哈希逻辑**，
AC8 天然满足。

## 4. 与 R5（运行时卷化）的边界

| | 本重构（子任务 1） | R5（子任务 2） |
|---|---|---|
| 改动对象 | `scenarios/`、`Dockerfile.base*`、`.aio/enabled.toml` | `app/entrypoint.sh`、`docker-compose.yml` 卷配置 |
| 生效时机 | 构建时 | 容器启动时 |
| 回滚 | 还原 scenario 目录 + 重新 build | 还原 entrypoint + 重建容器 |

**顺序**：子任务 1 先做（R5 依赖 L1 engine 已独立成 scenario，否则 symlink 种子的
目标路径不明确）。

**R5 的技术可行性已在规划期实测**（详见子任务 2 的 design.md）：
卷内 symlink 指向 `/opt/mise/installs/*`，`MISE_DATA_DIR` 指向卷，卷仅占 3.7M。

## 5. 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| 构建期联网失败（aqua 后端要走 GitHub/npm） | 构建挂起/失败 | 沿用现有 mise 场景的 `--mount=type=secret,id=github-token` 模式 |
| shims 路径与硬编码 `/usr/bin` 的脚本冲突 | 脚本断裂 | 全仓 grep 硬编码路径；现有 shell-utils 的 `ln -sf` 处理随拆分删除 |
| 场景目录从 8 → ~24，catalog 变长 | TUI/UI 列表变长 | 按 layer 分组已缓解；确认 TUI 滚动可用 |
| rust 1.4G 仍是最大单体 | 镜像体积 | 已由「按条可选」解决——不选就不装 |
| claude-code / codex 分发合规 | 法律风险 | Q3，提示用户确认，不阻塞实现 |
