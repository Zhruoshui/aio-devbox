# PRD：L3 层全量替换为 mise 管理方案

> 2026-09-03 修订：采纳 issue #6 评审意见——可见性改 ENV 主导（R1）、补
> `MISE_CONFIG_DIR` 第四 env（R1）、rust 显式 `profile = "default"`（R1）、移除
> 「运行时装到卷」逃生门（Out of Scope）、粒度坍缩显式取舍（Out of Scope）、
> AC 补非 login shell 与旧卷负向验证。

## Goal

将 sandbox-base 的 L3（lang 层）开发工具链安装方式从「每场景手写 fragment」统一替换为
「mise 作为安装引擎」，实现：一个场景装一个管理器，所有语言工具链的版本管理收口到
mise 一处。同时保持现有离线部署模型（make save/load + 三原语补装）不被打破。

## 背景与动机

- 现状 L3 有 5 个手写场景（go/nvm/python-dev/rust/uv），各自维护 curl/rustup/apt 安装
  配方，新增语言或升版本都要写一遍 fragment。
- mise PoC（`mise-poc/FINDINGS.md`，2026-09-02）已验证：mise 可在容器内运行时安装
  rust+opencode（Phase A）、可作 build-time 安装引擎且通过断网+卷遮盖双条件验证
  （Phase B）、离线可通过整目录搬迁分发（Phase C）。
- 本任务是 PoC 结论的固化实施，范围从原「新增 mise 场景」升级为「L3 全量收编」。

## Requirements

### R1 新增 mise 场景（唯一的新 L3 场景）
- 新建 `scenarios/mise/`（category=lang，非 always_on），承载全部 L3 职责：
  - mise 二进制 → `/usr/local/bin/mise`（版本经 ARG 固定）；
  - 烘焙期以 mise 安装原 L3 场景承载的工具链：rust、go、uv、ruff（版本各经 ARG 固定，
    对齐原场景当前版本：rust 1.93.1 或 stable、go 1.23.4、uv 0.5.11、ruff 0.8.4）；
  - 全部数据与配置落系统路径：`MISE_DATA_DIR=/opt/mise` + `MISE_CONFIG_DIR=/opt/mise`
    + `RUSTUP_HOME=/opt/mise/rustup` + `CARGO_HOME=/opt/mise/cargo`（四个 env 一起
    重定向，躲共享卷对 /root 与 /root/.config 的遮盖；config 一路是 PoC 盲区，
    由 issue #6 评审发现，见 design.md D1）；
  - rust 显式 `profile = "default"`（mise 缺省走 rustup minimal，会丢 clippy/rustfmt）
    + 显式补 `rustup component add rust-analyzer`（任何 profile 都不含，否则
    shim↔代理死循环）；
  - 可见性双保险（对齐原 rust/go 场景「ENV + symlink」手法，symlink 农场换成
    shims 目录；被收编的 6 个场景里 5 个现状是全 shell 可见，不是「与 nvm 同病」）：
    - Dockerfile `ENV` 烘入四个 env + `PATH=/opt/mise/shims:$PATH`——容器内全部
      进程继承（含 code-server 非 login 终端、被 spawn 的非交互子进程）；
    - `/etc/profile.d/mise.sh` 导出四个 env + `eval "$(mise activate bash)"`，
      补偿 login shell 被 `/etc/profile` 重置 PATH（WebUI 终端面板 `bash -l`）。

### R2 删除被收编的旧场景
- 删除 `scenarios/{rust,go,nvm,uv,python-dev}/` 五个目录。
- `.aio/enabled.toml` / presets 无需结构变更（`["*"]` 通配自动纳入新场景并排除已删场景；
  显式 id 列表如引用旧 id 会在 gen 时报错，属于期望的失败模式）。

### R3 opencode 同步收编（L4 附带）
- 删除 `scenarios/opencode/`，其二进制改由 mise 场景一并烘焙（PoC 验证过的
  `aqua:anomalyco/opencode` 后端）。
- 已知 upstream 从 sst 变更为 anomalyco，需在实现期验证产物等价性（`opencode --version`）。
- WebUI 面板按钮（app/services.toml 的 type=agent cmd=opencode）行为不变——按钮可见性
  本就由 command_exists 探测决定。

### R4 版本选择 UX 保持可用
- mise 场景不做 TUI [[versions]] 版本下拉（版本集合是「多工具联合体」，下拉无意义）；
  版本升级 = 编辑 fragment 顶部的 ARG 块（一行一个工具），在 scenario.toml description
  中注明升级方式。

### R5 离线兼容（不可打破的约束）
- `make save`/`load` 整机分发：mise 及其烘焙的工具随镜像走，零新增外部依赖。
- 离线补装新工具/新版本：沿用 PoC Phase C 验证的整目录搬迁配方（详见 design.md），
  并将该配方写入 `docs/offline-tool-install.md` 新章节。
- 离线机器上 mise 的 auto_install 必须关闭（`mise settings set auto_install false`
  在烘焙期完成），避免断网时静默 hang/下载。

### R6 CI 适配
- `.github/workflows/images.yml` full 变体 probe 列表更新：移除已删场景代表工具
  （nvm 等），加入 mise 及收编后仍需在 login shell 可见的代表命令。

### R7 文档
- `.claude/skills/aio-env-config/references/*` 中涉及被删场景的内容同步更新
  （layers.md 示例表、scenario-authoring.md 引用、recipes.md 示例）。
- `mise-poc/FINDINGS.md` 标注「已固化，见 scenarios/mise」。

## 明确不做（Out of Scope）

- 不动 L1 always_on 的 node/python 场景（app web-builder 与 code-server 构建依赖）。
- 不动 c23 场景（apt.llvm.org 系统级 C 工具链，mise registry 无对应、也不该走 registry）。
- 不动 L2 shell-utils、pi/pi-web（L4，npm 生态有自管理逻辑）。
- 不做「镜像种子→卷活态」的运行时拷贝设计，也不提供「运行时装工具到卷」的逃生门
  （issue #6 点 3：按调用覆盖 `MISE_DATA_DIR` 与全局 config 跨 data dir 共享冲突，
  回普通 shell 误触发 auto_install，且 shims PATH 化后卷上 shims 目录不在 PATH——
  与「种子→活态」同类问题，一并留待后续任务。本任务运行时 `mise use` 落容器可写层、
  recreate 即丢的取舍见 design.md D4）。
- 不做场景内工具子集开关（粒度坍缩已接受为取舍：启用 mise = 五工具全家桶含
  1.4GB rust，镜像无法按工具裁剪，见 design.md D9）。
- 不改 aio-config TUI/gen 的代码（场景机制原样使用）。

## Acceptance Criteria

- [ ] `make gen`（minimal 与 full preset）生成的 Dockerfile.base 不再含已删场景 fragment，
      含 mise 场景 fragment。
- [ ] `make build-base` 构建成功；镜像内 login shell（`bash -lc`）可找到并执行：
      `mise rustc cargo rustfmt rust-analyzer go gofmt uv ruff opencode`。
- [ ] `bash -lc 'cargo clippy --version'` 可用（rustup 组件完整性）。
- [ ] 断网容器 + 空白卷遮盖 /root 双条件下（复现 PoC Phase B v2 验证法），
      上述工具全部可用、无网络请求、无 auto_install 触发。
- [ ] 非 login shell（`bash -c`，无 -l）下上述工具同样全部可用（ENV+shims 通道，
      覆盖 code-server 非 login 终端与被 spawn 的非交互子进程）。
- [ ] 负向验证（issue #6 点 5）：卷上预置一份不含 [tools] 的
      `/root/.config/mise/config.toml` 遮盖镜像内容时，烘焙工具仍全部可用、
      无 auto_install 触发（`MISE_CONFIG_DIR` 重定向生效）。
- [ ] 离线整目录搬迁配方实测通过：tar /opt/mise（含 config.toml，单 tar）→
      断网新容器同路径解压 → 工具全部可用（PoC Phase C 复现）。
- [ ] CI probe 列表与场景集一致；full 变体 CI 绿。
- [ ] `docs/offline-tool-install.md` 新增 mise 章节；技能 references 更新后无失效引用。
- [ ] 现有 `.aio/enabled.toml`（若存在本地副本）不残留已删场景 id。
