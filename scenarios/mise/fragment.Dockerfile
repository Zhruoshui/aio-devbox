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

# ── 通道 2:profile.d(login shell 补偿 + 卷优先探测)─────────────────
# 这是**探测式**的:每次 source 时判断工作区卷上是否已播种(见
# app/aio-mise-volume.sh),是则优先用卷,否则回落到镜像内的 /opt/mise。
#
# 为什么探测放在 profile.d 而不是让 entrypoint 改写它:profile.d 烘在
# sandbox-base 里,被每个派生镜像共享——app 容器播种一次,code-server 与
# vnc 的 login shell 也跟着受益。若在运行期改写,只能修好改写者自己那个容器。
#
# 为什么需要「卷优先」:卷化后用户 `mise use -g` 装的东西才跨 recreate 存活
# (否则落容器可写层,recreate 即丢)。烘焙工具不受影响——卷上的树是 symlink,
# 指向的正是镜像里的 /opt/mise。
#
# 两个分支都用 `: "${HOME:=/root}"` 兜底:HOME 在 profile.d 被 source 的时机
# 由 /etc/profile 设好,但显式兜底更稳(与 ENV 通道的 /root 一致)。
RUN cat > /etc/profile.d/mise.sh <<'MISEPROFILE'
# mise activation for login shells (bash -l), scenario: mise.
# ENV channel covers non-login shells; this compensates /etc/profile
# resetting PATH in login shells (AIO terminal panel runs a pty bash -l).
#
# Prefer the volume-seeded data dir when present (seeded at app-container boot
# by app/aio-mise-volume.sh); otherwise fall back to the baked image layout.
# This file is baked into sandbox-base and shared by every derived image, so
# ONE seeding (by the always-started app container) makes the volume live for
# code-server's and vnc's shells too.
#
# Volume mode is what lets a user's runtime `mise use -g <tool>` survive
# recreate. Baked tools are unaffected: the volume's trees are symlinks into
# the image's /opt/mise.
if [ -d "${HOME:-/root}/.local/share/mise/installs" ]; then
	# Volume mode. MISE_CONFIG_DIR moves here too: the shims consult exactly
	# ONE config file, and it must list baked AND user tools together.
	# app/aio-mise-volume.sh regenerates $VOL/config.toml on every boot from
	# the image's (authoritative for baked tools) plus the user's additions.
	#
	# Note MISE_GLOBAL_CONFIG_FILE is deliberately NOT set: it REPLACES
	# MISE_CONFIG_DIR/config.toml rather than layering on top of it, so setting
	# both would hide the baked [tools] list and every baked tool would fail
	# with "No version is set for shim" (verified 2026-09-22).
	MISE_DATA_DIR="${HOME:-/root}/.local/share/mise"
	MISE_CONFIG_DIR="${HOME:-/root}/.local/share/mise"
	RUSTUP_HOME="${HOME:-/root}/.local/share/mise/rustup"
	CARGO_HOME="${HOME:-/root}/.local/share/mise/cargo"
else
	# Baked-only mode (no workspace volume, e.g. `docker run --rm <base> bash`).
	MISE_DATA_DIR=/opt/mise
	MISE_CONFIG_DIR=/opt/mise
	RUSTUP_HOME=/opt/mise/rustup
	CARGO_HOME=/opt/mise/cargo
fi
export MISE_DATA_DIR MISE_CONFIG_DIR RUSTUP_HOME CARGO_HOME
eval "$(mise activate bash)"
# Re-append the shim dirs AFTER activate, deliberately. activate rewrites PATH
# to the per-tool install dirs of the CURRENTLY activated tool set and drops
# the shims dirs entirely (verified 2026-09-22: 1 entry before, 0 after). That
# makes a tool installed during this very shell session invisible until the
# next activate — so `mise use -g X && X` would fail. Keeping the shims dirs
# on PATH restores it; they are only a fallback, since activate's dirs come
# first. Volume shims rank above the image's baked shims.
PATH="$MISE_DATA_DIR/shims:/opt/mise/shims:$PATH"
export PATH
MISEPROFILE

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
