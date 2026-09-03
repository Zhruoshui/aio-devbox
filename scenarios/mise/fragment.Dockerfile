# >>> scenario: mise >>>
# L3 lang 层统一场景:mise 作为安装/版本管理引擎,一次性烘焙原 rust/go/nvm/
# uv/python-dev(L3)与 opencode(L4 附带收编)五个场景承载的全部工具链。
# 实测依据 mise-poc/FINDINGS.md(Phase A/B/C,2026-09-02)。
#
# 关键布局约束(卷遮盖防护,同原 scenarios/rust 的 /opt/rust 模式):
#   共享卷 aio_workspace 挂 /root,镜像里落 /root 下的一切都会被遮盖。
#   mise 默认数据目录 ~/.local/share/mise、全局 config ~/.config/mise、
#   core:rust 的工具链 ~/.rustup / ~/.cargo —— 四处全部重定向到 /opt/mise
#   (镜像层,对所有派生容器可见)。installs 内部是绝对路径 symlink,
#   整目录搬迁到离线机时必须保持同路径(见 docs/offline-tool-install.md)。
#
# 可见性双保险(对齐原 rust/go 场景的「ENV + symlink」手法,symlink 农场
#   换成 shims 目录):
#   1) ENV 通道:四个重定向 env + shims PATH 烘进镜像元数据,容器内全部
#      进程继承 —— 覆盖非 login shell、非交互子进程、code-server 终端;
#   2) profile.d 通道:/etc/profile.d/mise.sh 重新导出四个 env 并
#      eval "$(mise activate bash)",补偿 login shell(bash -l)被 /etc/profile
#      重置 PATH(AIO WebUI 终端面板即 pty bash -l)。activate 的 hook-env
#      实时计算动态环境(core:rust 的 RUSTUP_TOOLCHAIN 等),比静态 PATH 更正确。

ARG MISE_VERSION=v2026.9.0
ARG RUST_VERSION=1.93.1
ARG GO_VERSION=1.23.4
ARG UV_VERSION=0.5.11
ARG RUFF_VERSION=0.8.4
ARG OPENCODE_VERSION=1.18.24

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

# auto_install 默认开启:缺工具时 activate 会静默发起下载(离线机表现为
# hang/DNS 报错)。在 config.toml 写 [settings] 段关闭,缺工具显式报错。
# 注意 mise settings 子命令读写的正是 MISE_CONFIG_DIR/config.toml 的这一段
# (实测;先 set 再重写整个 config.toml 会把设置抹掉)。MISE_OFFLINE 不设为
# 镜像级默认(在线机器保留自动补装体验,离线机由用户/文档显式设)。

# ── 全局 [tools] 清单(写进 MISE_CONFIG_DIR,随镜像走)─────────────────
# rust 必须显式 profile="default":mise 的 core:rust 走 rustup,全新
# RUSTUP_HOME 的 rustup 默认 profile 是 minimal(仅 rustc/rust-std/cargo),
# 会丢 clippy/rustfmt —— 对齐原 scenarios/rust 的 --profile default。
# rust-analyzer 不在任何 profile 里,单独 component add(缺组件时 rustup
# 代理沿 PATH fallback 撞 shim,shim 再指回代理 → 死循环,PoC 实测)。
RUN mkdir -p /opt/mise \
 && printf '[settings]\nauto_install = false\n\n[tools]\nrust = { version = "%s", profile = "default" }\ngo = "%s"\nuv = "%s"\nruff = "%s"\nopencode = "%s"\n' \
      "${RUST_VERSION}" "${GO_VERSION}" "${UV_VERSION}" "${RUFF_VERSION}" "${OPENCODE_VERSION}" \
      > /opt/mise/config.toml \
 && mise settings get auto_install | grep -qx false \
 && mise install \
 && mise exec -- rustup component add rust-analyzer \
 && mise ls

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
RUN bash -lc 'for t in mise rustc cargo rustfmt rust-analyzer go gofmt uv ruff opencode; do command -v "$t" >/dev/null || { echo "MISSING(login): $t" >&2; exit 1; }; done' \
 && bash -lc 'cargo clippy --version' \
 && bash -c 'for t in mise rustc cargo rustfmt rust-analyzer go gofmt uv ruff opencode; do command -v "$t" >/dev/null || { echo "MISSING(non-login): $t" >&2; exit 1; }; done' \
 && bash -c 'go version && uv --version && ruff --version && opencode --version'

# ── 体积优化:丢弃构建期下载缓存(运行时不需要;离线搬迁配方搬的是整个
#    data dir,不含 downloads 也成立 —— installs 已就位)─────────────────
RUN rm -rf /opt/mise/downloads
# <<< scenario: mise <<<
