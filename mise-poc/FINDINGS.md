# mise PoC 实测记录（2026-09-02）

> 先验性测试结论。mise 版本 **v2026.9.0**，基座镜像 `sandbox-base:latest`（c1e4f5a4a0f9）。
> 三个 Phase 全部在一次性容器中完成，未触碰 `.aio/enabled.toml`、现有场景与运行中的 stack。
> 复现实验：`docker build -f mise-poc/Dockerfile -t mise-poc-baked:v2 .`（约 90s）。

## 结论速览

| Phase | 问题 | 结论 |
|---|---|---|
| A 运行时 | mise 在容器内装 rust + opencode | ✅ 15s/18s 装完，login shell 全绿，落盘卷语义路径 |
| B 烘焙 | mise 作为 build-time 安装引擎 | ✅ `MISE_DATA_DIR=/opt/mise` + rustup 双家目录重定向后，断网+卷遮盖双条件全绿 |
| C 离线 | MISE_OFFLINE + cache 预填 | ✅ 但姿势必须是**整目录搬迁**，downloads cache 单独搬无效 |

**总判定：mise 可与本项目有机结合，离线部署不会被打破——前提是遵守下述五条实测约束。**

## 关键发现（按重要性排序）

### 1. mise core:rust 是 rustup 的"收编器"，不是替代品 ⚠️ 最大的坑

mise 的 rust core 插件内部跑 rustup：真实工具链落在 `RUSTUP_HOME`（默认 `~/.rustup`，1.4GB），
`installs/rust/<ver>` 只是指向 `~/.cargo/bin` 的 **symlink**。

- 不重定向直接烘焙 → 运行时 `/root` 被 `aio_workspace` 卷遮盖 → symlink 悬空 →
  `mise activate` 触发 **auto_install 静默重新下载 1.4GB 进卷**（在线能自愈，离线当场翻车）。
- 修法（已验证）：烘焙时同时设
  `MISE_DATA_DIR=/opt/mise` + `RUSTUP_HOME=/opt/mise/rustup` + `CARGO_HOME=/opt/mise/cargo`，
  与现有 `scenarios/rust` 的 `/opt/rust` 模式同构。

### 2. rust 默认缺 rust-analyzer，且缺的方式会死循环

rustup `default` profile 不含 rust-analyzer（现有 rust 场景是显式 `--component` 装的）。
缺组件时 rustup 代理沿 PATH fallback 撞上 mise shim，shim 再指回代理 → **infinite recursion**。
烘焙流程必须显式 `mise exec -- rustup component add rust-analyzer`。

### 3. 离线的正确姿势是"整目录搬迁"，不是 cache 预填

- `MISE_OFFLINE=1` 是**硬失败**（"offline mode is enabled"），不会回退到 downloads cache；
- `downloads/` cache 单独搬到离线机**没用**：aqua 后端照样报 offline error；
  core:rust 的 rustup 另有一套 cache（`rustup/downloads/`），也不认 mise 的；
- 实测通过的配方（三步，对应现有离线三原语）：
  1. 联网机正常 `mise install`，然后
     `tar -cf mise-bundle.tar -C <MISE_DATA_DIR父目录> <MISE_DATA_DIR目录名>`；
     同时带走 `~/.config/mise/config.toml`（全局工具清单，否则 shim 报 "No version is set"）；
  2. 传输到离线机；
  3. **解压回与联网机完全相同的绝对路径**（mise installs 内部是绝对路径 symlink，
     换路径即断，如 `.mise-bins/fd -> /opt/mise/installs/fd/...`），设 `MISE_OFFLINE=1`。
- mise 二进制本身 = 单文件，现有"单二进制 → `~/.local/bin`"配方直接适用，无新增依赖。

### 4. auto_install 默认开启，离线环境必须关掉

`mise settings set auto_install false`（默认 true）。不关的话，缺工具时 activate 会静默
发起下载——离线机上表现为 hang 或 DNS 报错，且行为不显式。

### 5. registry 元数据本地内置，ls-remote 才走网络

`mise registry rust` / `mise registry opencode` 本地即时返回。这意味着离线机上
版本解析、`mise ls` 等管理操作零网络依赖，只有 `install`/`ls-remote` 需要网。

## 环境事实（后续固化要用）

- 最新版 v2026.9.0；tarball 结构 `mise/bin/mise`（注意：不是根目录）；
  URL 模板 `https://github.com/jdx/mise/releases/download/v{V}/mise-v{V}-linux-x64.tar.gz`
- rust 走 `core:` 插件（预编译，15s）；opencode 走 `aqua:anomalyco/opencode`
  （注意 upstream 是 anomalyco 不是现有场景的 sst，18s，含 checksum）
- 全套体积：mise 二进制 ~30MB + rust 工具链 1.4GB + opencode ~100MB ≈ 共 1.5GB
- `mise activate bash` 注入较重（PROMPT_COMMAND/cd hook/command_not_found_handle），
  对 AIO 终端面板（pty login shell）适用；CI probe 用 `bash -lc + command -v` 可通过
- `/root/.config/mise/config.toml` 烘焙后首次挂空白命名卷会被 Docker 拷进卷（copy-on-first-use），
  旧卷则被用户自己的 config 遮盖——两者语义都正确
- 沙箱网络策略下所需域名全部可达：github.com releases、mise.jdx.dev、api.github.com、registry.npmjs.org

## 固化建议（如果决定引入）

1. **新增 `scenarios/mise/`**（L3 `lang`，非 always_on）：二进制 → `/usr/local/bin`，
   `/etc/profile.d/mise.sh` 做 activate + 三个家目录 env 导出。
   运行时用户自己 `mise use X` 装的额外工具落 `~/.local/share/mise`（卷，抗 recreate）——
   与 nvm/uv hybrid 模式完全一致。
2. **rust/opencode 是否收编为 mise 管理**是独立决策：
   - 收编收益：安装配方统一为 `mise use <tool>@<ver>`，版本选择可交给 mise.toml；
   - 收编代价：rust 场景从 1 个 RUN 变成"mise + 家目录重定向 + component add"三件事，
     opencode upstream 从 sst 变 anomalyco（需确认是否可接受）；
   - 建议：先只加 `scenarios/mise/` 作为**运行时版本管理器**（对位 nvm/uv），
     烘焙引擎收编等真实需求出现再评估。
3. **离线文档增补**：`docs/offline-tool-install.md` 加"mise 整目录搬迁"一节
   （上述三步 + 同路径约束 + config.toml 必带 + auto_install=false）。
4. **CI**：`["*"]` 预设会自动带上新场景；probe 列表加 `mise --version`。

## 清理状态

所有 PoC 容器/卷/镜像/临时 tar 已删除；仅保留本文件 + `Dockerfile`（90s 可重建）。
