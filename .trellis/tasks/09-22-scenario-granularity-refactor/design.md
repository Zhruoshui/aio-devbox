# design.md — Scenario 粒度重构

> 父任务的架构总览见 `.trellis/tasks/09-22-scenario-items-layered-mise/design.md`。
> 本文件只记录**本子任务的实现级设计**。

## 1. 核心机制：为什么 `gen` 不用改

### 1.1 实测依据

三个独立 RUN 层各跑 `mise use -g <tool>`，最终 `/opt/mise/config.toml`：

```toml
[tools]
fd = "10.5.0"
jq = "1.8.2"
starship = "1.26.0"
```

`mise use -g` 是**读改写**语义：读现有 config → 合并 `[tools]` → 写回。
对 Docker 顺序层天然幂等。

**结论**：`gen` 的纯字符串拼接模型完全适用，无需聚合、无需依赖图。

### 1.2 fragment 假定 engine 已就绪

每个工具 fragment 都假定 `/opt/mise` 与 `MISE_*` env 已存在。这**不是构建期依赖**，
而是 **layer 排序保证**：`gen::sort_by_layer` 把 `category = "os"` 排在
`shell`/`lang`/`app` 之前，engine 的 RUN 层必然先执行。

若排序被破坏，fragment 会因 `mise: command not found` 快速失败——可接受的 fail-fast。

## 2. 三个 fragment 模板

### 模板 1 — 标准 mise 工具（L2 全部 + L3 的 go/uv/ruff + L4 的 agent）

```dockerfile
# >>> scenario: <id> >>>
# <一行说明>
ARG <TOOL>_VERSION=<ver>
RUN mise use -g "<tool>@${<TOOL>_VERSION}" \
 && bash -lc "command -v <bin>" \
 && <bin> --version
# <<< scenario: <id> <<<
```

要点：
- `bash -lc` 自检走 **login shell**，覆盖 WebUI 终端面板的 pty 路径
- `ARG` 在 fragment 内声明（非文件顶部全局），保证版本可复现
- banner 注释 `# >>> scenario: id >>>` / `# <<< scenario: id <<<` 必须保留
  （`gen` 只用它们做分隔，但保持了可读性）

### 模板 2 — rust（唯一需要 table 形式的）

`mise use -g` 只能写 `rust = "1.93.1"` 简单形式，但 rust 需要
`profile = "default"`（否则丢 clippy/rustfmt）。**必须直接写 config**：

```dockerfile
# >>> scenario: rust >>>
ARG RUST_VERSION=1.93.1
RUN printf 'rust = { version = "%s", profile = "default" }\n' "${RUST_VERSION}" \
      >> /opt/mise/config.toml \
 && mise install rust \
 && mise exec -- rustup component add rust-analyzer \
 && bash -lc 'for t in rustc cargo clippy rustfmt rust-analyzer; do command -v "$t" >/dev/null || exit 1; done' \
 && bash -c 'cargo clippy --version'
# <<< scenario: rust <<<
```

**两个补偿缺一不可**：
| 补偿 | 缺失后果 |
|---|---|
| `profile = "default"` | rustup 默认 minimal profile，丢 clippy / rustfmt |
| `rustup component add rust-analyzer` | rust-analyzer 不在任何 profile 里；**缺失时 rustup 代理沿 PATH 撞上 mise shim，shim 再指回代理 → 无限递归**（PoC 实测） |

`printf ... >> config.toml` 用**追加**而非覆盖，保证与其他 fragment 可组合
（`mise install rust` 会读取完整 config 只装 rust）。

### 模板 3 — c23（apt，不动）

保持 `scenarios/c23/fragment.Dockerfile` 原样。

## 3. L1 engine 拆分

从现有 `scenarios/mise/fragment.Dockerfile` 提取：

| 保留 | 删除 |
|---|---|
| `ARG MISE_VERSION` | `ARG RUST_VERSION` 等 5 个工具版本 |
| mise 二进制 tarball 安装 | `[settings]` / `[tools]` 段写入 |
| 四个 ENV 重定向 | `mise install` |
| `/etc/profile.d/mise.sh` | `rustup component add rust-analyzer` |
| 双通道自检（但只验 `mise`） | `rm -rf /opt/mise/downloads` |

新增：
- `scenario.toml` 的 `category = "os"`、`always_on = true`
- 自检改为 `bash -lc 'command -v mise'` + `bash -c 'mise --version'`（双通道各一遍）
- **不创建** `/opt/mise/installs`（留给工具 fragment 首次使用时创建）
  —— 但 `MISE_DATA_DIR=/opt/mise` 需已 `mkdir -p`，否则 mise 写 config 失败

**注意**：engine fragment 仍需 `mkdir -p /opt/mise`（现有做法），
但**不写 config.toml 的 `[tools]` 段**。工具 fragment 用 `>>` 追加。

## 4. pi 的 mise 化

### 4.1 改动点

`scenarios/pi/fragment.Dockerfile` 中：

```dockerfile
# 改前
RUN npm install -g --ignore-scripts "@earendil-works/pi-coding-agent@${PI_VERSION}" && pi --version

# 改后
RUN mise use -g "pi@${PI_VERSION}" && bash -lc 'command -v pi' && pi --version
```

### 4.2 必须原样保留（一行都不能动）

- `COPY scenarios/pi/pi-packages/package.json /opt/pi-extensions/package.json`
- `/opt/pi-extensions` 的 `npm install --omit=dev`
- `agent-browser` CLI 烘焙 + 非本平台二进制裁剪 + shim 改名 + wrapper
- `pi-agent-browser-doctor` / `pi-agent-browser-config` 软链
- `COPY scenarios/pi/aio-pi-extensions.sh` + `chmod` + `bash -n`
- `COPY scenarios/pi/agent-browser-wrapper.sh` + `chmod` + `bash -n`

**版本注意**：现状 npm 装 `0.84.2`，mise aqua 提供 `0.86.1`。
本任务**锁 `PI_VERSION=0.84.2`** 以对齐现有 agent-browser 插件基线
（用户说「先不管」，故不主动升级）。

### 4.3 `pi-web` 不动

`scenarios/pi-web/` 保持 npm 安装（Next.js 应用，非 CLI，不属于本次范围）。

## 5. 契约不变清单

| 模块 | 改动 |
|---|---|
| `config/src/scenario.rs` | **不变** |
| `config/src/gen.rs` | **不变** |
| `config/src/manifest.rs` | **不变** |
| `config/src/tui.rs` | **不变** |
| `mgr/src/envhash.rs` | **不变** |
| `mgr-web/src/types.ts` | **不变** |
| `mgr-web/src/pages/EnvPicker.tsx` | **不变**（新 agent 不在 `SERVICE_SCENARIOS`） |

**这意味着本子任务的绝大部分工作是新增 scenario 目录**——低风险、可逐个验证。

## 6. 目录命名与版本号

| scenario id | 版本 | 说明 |
|---|---|---|
| `mise` | MISE_VERSION=v2026.9.0 | engine（复用原目录） |
| `rust` | 1.93.1 | 模板 2 |
| `go` | 1.23.4 | 模板 1 |
| `uv` | 0.5.11 | 模板 1 |
| `ruff` | 0.8.4 | 模板 1 |
| `fzf` | 0.74.4 | 实测装出 |
| `ripgrep` | 15.2.0 | 实测装出（注意：不是 14.x） |
| `bat` | 0.26.1 | 实测装出 |
| `fd` | 10.5.0 | 实测装出 |
| `eza` | 0.23.5 | 实测装出 |
| `zoxide` | 0.10.0 | 实测装出 |
| `delta` | 0.19.2 | 实测装出 |
| `starship` | 1.26.0 | 实测装出 |
| `jq` | 1.8.2 | 实测装出 |
| `yq` | 4.53.6 | 实测装出 |
| `opencode` | 1.18.24 | 沿用原 mise 场景 |
| `claude-code` | 2.1.278 | 实测装出 |
| `codex` | 0.155.1 | 实测装出 |

**版本号均来自 2026-09-22 实测**，非猜测。

## 7. 风险

| 风险 | 缓解 |
|---|---|
| 构建期联网失败（aqua 走 GitHub/npm） | 保持 `--mount=type=secret,id=github-token` 可选注入；local 构建不传则落空 |
| `ripgrep` 的 bin 名是 `rg` 非 `ripgrep` | 自检写 `command -v rg` |
| `bat` 的 bin 名是 `bat`（mise 非 Debian） | 无 `batcat` 问题 |
| `fd` 的 bin 名是 `fd` | 无 `fdfind` 问题 |
| 场景目录 8 → ~24，TUI 列表变长 | 按 layer 分组已缓解 |
