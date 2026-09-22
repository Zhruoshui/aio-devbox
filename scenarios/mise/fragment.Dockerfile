# >>> scenario: mise >>>
# L1 os 层:mise **engine** 本体(always_on)。只提供工具链管理能力,不含任何工具。
#
# 为什么 engine 与工具分离(2026-09-22 粒度重构):
#   原 `scenarios/mise` 是 all-or-nothing 的五工具全家桶(~1.5GB,含 rust 1.4GB),
#   想用 uv 也得吃下 rust。重构后 engine 常驻 L1(~30MB),工具各自成为独立
#   scenario(rust/go/uv/ruff 在 L3,shell 工具在 L2,agent 在 L4),按需勾选。
#
# 为什么不需要聚合机制(设计依据,实测):
#   `mise use -g <tool>` 是读改写语义——读现有 config → 合并 [tools] → 写回。
#   因此多个 fragment 各自追加工具,在 Docker 顺序层上是幂等可组合的:
#   三个独立 RUN 层各装一个工具后,/opt/mise/config.toml 的 [tools] 段是三者并集,
#   互不覆盖(2026-09-22 实测:fd + jq + starship 三者共存)。
#   gen 因此保持纯字符串拼接,无需任何聚合或依赖图。
#
# 层序前提:工具 fragment 假定本 fragment 已执行(需要 /opt/mise 与四个 MISE_* env)。
#   这不是构建期依赖,而是 layer 排序保证——gen 的 sort_by_layer 把 os 层排在
#   shell/lang/app 之前。排序若被破坏,工具 fragment 会因 mise: command not found
#   快速失败(fail-fast,不留隐性错误)。
#
# 关键布局约束(卷遮盖防护,沿用原设计):
#   共享卷 aio_workspace 挂 /root,镜像里落 /root 下的一切都会被遮盖。
#   mise 默认数据目录 ~/.local/share/mise、全局 config ~/.config/mise、
#   core:rust 的工具链 ~/.rustup / ~/.cargo —— 四处全部重定向到 /opt/mise
#   (镜像层,对所有派生容器可见)。installs 内部是绝对路径 symlink,
#   整目录搬迁到离线机时必须保持同路径(见 docs/offline-tool-install.md)。
#
# 可见性双保险(对齐原设计的「ENV + profile.d」双通道):
#   1) ENV 通道:四个重定向 env + shims PATH 烘进镜像元数据,容器内全部
#      进程继承 —— 覆盖非 login shell、非交互子进程、code-server 终端;
#   2) profile.d 通道:/etc/profile.d/mise.sh 重新导出四个 env 并
#      eval "$(mise activate bash)",补偿 login shell(bash -l)被 /etc/profile
#      重置 PATH(AIO WebUI 终端面板即 pty bash -l)。activate 的 hook-env
#      实时计算动态环境(core:rust 的 RUSTUP_TOOLCHAIN 等),比静态 PATH 更正确。

ARG MISE_VERSION=v2026.9.0

# ── 通道 1:ENV(非 login shell / 非交互子进程全覆盖)───────────────────
ENV MISE_DATA_DIR=/opt/mise \
    MISE_CONFIG_DIR=/opt/mise \
    RUSTUP_HOME=/opt/mise/rustup \
    CARGO_HOME=/opt/mise/cargo \
    PATH=/opt/mise/shims:$PATH

# ── mise 二进制本体 ────────────────────────────────────────────────────
# tarball 结构是 mise/bin/mise(不是根目录)。单文件,无运行时依赖。
RUN curl -fsSL "https://github.com/jdx/mise/releases/download/${MISE_VERSION}/mise-${MISE_VERSION}-linux-x64.tar.gz" -o /tmp/mise.tar.gz \
 && tar -xzf /tmp/mise.tar.gz -C /tmp \
 && install -m 0755 /tmp/mise/bin/mise /usr/local/bin/mise \
 && rm -rf /tmp/mise /tmp/mise.tar.gz \
 && mise --version

# ── 数据目录与 config 骨架 ─────────────────────────────────────────────
# MISE_DATA_DIR 必须存在,否则工具 fragment 追加 config 时写失败。
# 这里只创建 config 骨架并写入 [settings];[tools] 段由各工具 fragment 用
# `>>` 追加(见 fragment 顶部「为什么不需要聚合机制」)。
#
# auto_install 默认开启:缺工具时 activate 会静默发起下载(离线机表现为
# hang/DNS 报错)。在 config.toml 写 [settings] 段关闭,缺工具显式报错。
# 注意 mise settings 子命令读写的正是 MISE_CONFIG_DIR/config.toml 的这一段
# (实测;先 set 再重写整个 config.toml 会把设置抹掉)。
# MISE_OFFLINE 不设为镜像级默认(在线机器保留自动补装体验,离线机由用户/文档显式设)。
#
# 用 `>` 而非 `>>`:engine 是第一个写 config 的 scenario,负责建立文件;
# 后续工具 fragment 一律用 `>>` 追加。
RUN mkdir -p /opt/mise \
 && printf '[settings]\nauto_install = false\n\n[tools]\n' > /opt/mise/config.toml \
 && mise settings get auto_install | grep -qx false

# ── 通道 2:profile.d(login shell 补偿)───────────────────────────────
RUN printf '%s\n' \
      '# mise activation for login shells (bash -l), scenario: mise.' \
      '# ENV channel covers non-login shells; this compensates /etc/profile' \
      '# resetting PATH in login shells (AIO terminal panel runs a pty bash -l).' \
      'export MISE_DATA_DIR=/opt/mise' \
      'export MISE_CONFIG_DIR=/opt/mise' \
      'export RUSTUP_HOME=/opt/mise/rustup' \
      'export CARGO_HOME=/opt/mise/cargo' \
      'eval "$(mise activate bash)"' \
      > /etc/profile.d/mise.sh

# ── 自检:双通道各过一遍(安装期内失败即中止,不留隐性回归)──────────────
# engine 只验 mise 本身;工具可用性由各自 fragment 自检。
# installs 目录此处应为空(或不存在)——engine 不装任何工具。
RUN bash -lc 'command -v mise >/dev/null || { echo "MISSING(login): mise" >&2; exit 1; }' \
 && bash -c 'command -v mise >/dev/null || { echo "MISSING(non-login): mise" >&2; exit 1; }' \
 && bash -lc 'mise --version' \
 && bash -c 'mise --version' \
 && { [ ! -d /opt/mise/installs ] || [ -z "$(ls -A /opt/mise/installs 2>/dev/null)" ]; } \
      || { echo "engine fragment must not install tools" >&2; exit 1; }
# <<< scenario: mise <<<
