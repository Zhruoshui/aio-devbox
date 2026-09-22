# >>> scenario: rust >>>
# L3 lang 层:Rust 工具链,由 mise 的 core:rust 后端安装(内部跑 rustup)。
#
# 为什么不用 `mise use -g`(与其他 mise 工具不同):
#   `mise use -g` 只能写简单形式 `rust = "1.93.1"`,而 rust 需要
#   `{ version = "...", profile = "default" }` 这个 table 形式。故此处**直接
#   追加 config**,再显式 `mise install rust`。用 `>>` 而非 `>` 以与其他
#   fragment 可组合(mise use 本身也是读改写,两者在顺序层上等价)。
#
# 两个补偿缺一不可(PoC 实测,2026-09-02):
#   1) profile = "default":mise 的 core:rust 走 rustup,而全新 RUSTUP_HOME 的
#      rustup 默认 profile 是 minimal(仅 rustc/rust-std/cargo),会丢
#      clippy/rustfmt —— 对齐原 scenarios/rust 的 --profile default。
#   2) rust-analyzer 不在任何 profile 里,必须单独 component add。且**缺组件时
#      会死循环**:rustup 代理沿 PATH fallback 撞上 mise shim,shim 再指回代理
#      → infinite recursion。故这个 component add 不是可选项。
#
# 家目录:RUSTUP_HOME / CARGO_HOME 由 L1 mise engine fragment 重定向到
# /opt/mise(镜像层,躲共享卷遮盖),此处无需重复声明。
#
# installs/rust/<ver> 只是指向 RUSTUP_HOME 下真实工具链的 symlink ——
# 这层间接是「运行时卷化」方案必须一并处理 rustup/cargo 的原因。

ARG RUST_VERSION={{version}}
RUN printf 'rust = { version = "%s", profile = "default" }\n' "${RUST_VERSION}" \
      >> /opt/mise/config.toml \
 && mise install rust \
 && mise exec -- rustup component add rust-analyzer \
 && mise ls \
 && bash -lc 'for t in rustc cargo rustfmt rust-analyzer clippy-driver; do command -v "$t" >/dev/null || { echo "MISSING(login): $t" >&2; exit 1; }; done' \
 && bash -c 'command -v rustc >/dev/null || { echo "MISSING(non-login): rustc" >&2; exit 1; }' \
 && bash -lc 'rustc --version && cargo --version && cargo clippy --version && rustfmt --version && rust-analyzer --version'
# <<< scenario: rust <<<
