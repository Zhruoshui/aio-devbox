# Implement：L3 全量替换为 mise 管理方案

> 前置：design.md D1-D9 已定（2026-09-03 采纳 issue #6 评审修订）。执行顺序敏感：
> Step 2 的临时 Dockerfile 验证必须在 Step 3 改真实场景**之前**跑通（PoC 只验证过
> rust+opencode 且未测 MISE_CONFIG_DIR / activate+shims 并存，go/uv/ruff 首次走 mise）。

## Step 0 前置验证（5 分钟）

- [ ] `docker images | grep sandbox-base` 存在且为最新（`make gen` 后无漂移）。
- [ ] `mise ls-remote go | grep 1.23`、`mise ls-remote uv | grep 0.5`、
      `mise ls-remote ruff | grep 0.8` 有目标版本（用一个本地一次性容器验证，
      `docker run --rm -v $PWD:/w sandbox-base bash -lc '...'`，不装进镜像）。

## Step 1 写场景文件

- [ ] 新建 `scenarios/mise/scenario.toml`：
      id=mise, name="mise (L3 工具链统一管理器)", category=lang,
      description 注明「版本升级=编辑 fragment ARG 块」「启用即安装全部五工具
      （all-or-nothing，design.md D9）」与「运行时自装工具落容器可写层、
      recreate 即丢（design.md D4）」。
- [ ] 新建 `scenarios/mise/fragment.Dockerfile`（骨架，banner 格式对齐现有场景）：
      - ARG 块：MISE_VERSION=v2026.9.0, RUST_VERSION=1.93.1, GO_VERSION=1.23.4,
        UV_VERSION=0.5.11, RUFF_VERSION=0.8.4, OPENCODE_VERSION=1.18.24
      - ENV 四重定向 + shims PATH（design.md D2 双保险的 ENV 通道）：
        `MISE_DATA_DIR=/opt/mise MISE_CONFIG_DIR=/opt/mise RUSTUP_HOME=/opt/mise/rustup
        CARGO_HOME=/opt/mise/cargo PATH=/opt/mise/shims:$PATH`
      - RUN 安装 mise 二进制（GitHub release tarball, `mise/bin/mise` 路径）→ /usr/local/bin
      - RUN `mise settings set auto_install false`
      - RUN 写 config.toml：`[tools] rust = { version = "${RUST_VERSION}", profile = "default" }`
        + go/uv/ruff/opencode 各行（profile="default" 保证 clippy/rustfmt，design.md D1.1）
      - RUN `mise install`（rust 需紧跟 `mise exec -- rustup component add rust-analyzer`）
      - RUN 验证块：`bash -lc 'mise ls'` + 逐工具 `--version`（login shell 语义自检）
        + `bash -c`（非 login，ENV+shims 通道自检）+ `cargo clippy --version`
      - RUN 写 `/etc/profile.d/mise.sh`（四个 env + activate，补偿通道）
      - RUN 清理 `/opt/mise/downloads`（体积优化，验证不依赖它）

## Step 2 临时 Dockerfile 全量验证（改真实文件之前）

- [ ] 以 Step 1 的 fragment 内容为主体写 `mise-poc/Dockerfile.l3`（FROM sandbox-base），
      `docker build` 成功。
- [ ] config 落点首验（issue #6 点 5，最高优先）：构建后镜像内
      `/opt/mise/config.toml` 存在且含 [tools]，`/root/.config/mise/` **不**残留
      config.toml（MISE_CONFIG_DIR 生效）。
- [ ] 断网 + 空白卷双条件验证（复现 PoC Phase B v2 法）：
      `docker run --network none -v <新卷>:/root` → `bash -lc` 下
      `mise rustc cargo rustfmt rust-analyzer go gofmt uv ruff opencode` 全部 `command -v` 命中
      且 `--version` 输出正确、`cargo clippy --version` 可用。
- [ ] 同容器 `bash -c`（非 login，无 -l）复测同一命令集（ENV+shims 通道，
      activate 与 shims PATH 并存组合的首次实测）。
- [ ] 负向验证：先在新卷里预置一份不含 [tools] 的 `/root/.config/mise/config.toml`
      （模拟旧卷用户 config 遮盖），再启动断网容器——工具仍全部可用、无
      "No version is set"、无 auto_install 触发。
- [ ] 检查无 auto_install 网络尝试（auto_install=false 已关）。
- [ ] 失败处理：go/uv/ruff 任一后端异常 → 记录到本文件末尾「发现」小节，
      调整 fragment（必要时对该工具回退单二进制 curl 安装行，在 fragment 内注明原因）。

## Step 3 应用到真实场景树

- [ ] `git rm -r scenarios/rust scenarios/go scenarios/nvm scenarios/uv scenarios/python-dev scenarios/opencode`
- [ ] `mkdir scenarios/mise`（或 git mv 复用目录），放入 Step 1 文件。
- [ ] `make gen`（用临时 enabled.toml 或直接用 presets 验证两轮）：
      - minimal preset：Dockerfile.base 无 mise fragment；
      - full preset（`["*"]`）：含 mise fragment、不含已删场景 fragment。

## Step 4 构建与容器验证

- [ ] `make build-base`（真实 build，用户授权后）。
- [ ] 一次性容器 `docker run --rm sandbox-base bash -lc 'for t in mise rustc cargo
      rustfmt rust-analyzer go gofmt uv ruff opencode; do command -v $t; done'` 全命中。
- [ ] 同容器 `bash -c`（非 login）复测全命中（AC 的 ENV+shims 通道抽查，
      覆盖 code-server 非 login 终端语义）。
- [ ] `bash -lc 'cargo clippy --version && go version && uv --version && ruff --version
      && opencode --version'` 输出正确。
- [ ] 卷遮盖复验：新卷挂 /root 的容器里同样全命中（无悬空 symlink、无重下载迹象）；
      叠加「卷上预置无 [tools] config」负向验证（Step 2 同款）。

## Step 5 离线配方实测（PoC Phase C 复现）

- [ ] 联网容器（隔离验证，两 env 一致覆盖）：`MISE_DATA_DIR=/root/mise-x
      MISE_CONFIG_DIR=/root/mise-x mise use -g fd@10.2.0`（新工具），tar 整个
      `/root/mise-x`（data + config 单 tar）。
- [ ] 断网容器：同路径解压、同 env 覆盖 + `MISE_OFFLINE=1 mise install` →
      `fd --version` 可用。
- [ ] 烘焙的 /opt/mise 不受影响（隔离路径与镜像路径互不干扰确认）。

## Step 6 CI 与文档

- [ ] `.github/workflows/images.yml` probe 列表按 design.md D7 更新。
- [ ] `docs/offline-tool-install.md` 新增「mise 整目录搬迁」章节（配方三步 +
      同路径约束 + MISE_DATA_DIR/MISE_CONFIG_DIR 一致覆盖 + 单 tar 含 config +
      MISE_OFFLINE=1 + auto_install 已关说明；注明配方在隔离容器验证、运行中
      sandbox 混用卷路径属后续任务）。
- [ ] 技能 references 更新：layers.md L3 行示例表、scenario-authoring.md、
      recipes.md、paths-and-offline.md 中被删场景的引用。
- [ ] `mise-poc/FINDINGS.md` 顶部加「已固化」标注。

## Step 7 收尾

- [ ] 清理：删掉 `mise-poc/Dockerfile.l3` 验证残留容器/卷/镜像；`mise-poc/Dockerfile`
      （Phase B 的）可保留或删除，FINDINGS.md 里注明。
- [ ] 检查 `.aio/enabled.toml` 本地副本无已删 id（若存在）。
- [ ] 全 scope quality check（2.2 最后一轮）：gen 两轮 + probe 模拟 + 文档引用
      grep 检查（`grep -rn "scenarios/rust\|scenarios/nvm\|scenarios/uv\b" --include="*.md"` 无失效引用）。
- [ ] Trellis 3.3 spec update：若 .trellis/spec 有 backend/env 相关 index 条目提及旧场景，
      一并更新。

## 回滚点

- Step 2 失败 → 全部变更仅在 mise-poc/ 临时文件，直接丢弃即可。
- Step 3 之后失败 → `git checkout -- scenarios/ .aio/` 恢复场景树。
- 已 build 镜像不理想 → 旧镜像 tag 仍在本地（build 前记录 digest），可重新 tag。
