# >>> scenario: rust >>>
# Rust toolchain via online rustup (build machine has network; offline is
# handled by whole-image `docker save/load`, NOT by vendoring rustup here).
#
# Lands in /opt/rust (system path), NOT ~/.cargo: the workspace volume mounts
# over /root, so anything baked into /root/.cargo would be masked /
# go stale (卷遮盖). /opt/rust is in the image layer, visible in every
# container derived from sandbox-base, and RUSTUP_HOME/CARGO_HOME point there.
# Everything runs as root, so `cargo install` / `rustup` write freely.

ARG RUST_VERSION=stable
ENV RUSTUP_HOME=/opt/rust/rustup \
    CARGO_HOME=/opt/rust/cargo \
    PATH=/opt/rust/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile default --default-toolchain ${RUST_VERSION} \
                 --component rust-analyzer \
 && rustup default ${RUST_VERSION} \
 && rustc --version && cargo --version && rustfmt --version && cargo clippy --version \
# rustup puts its proxies in /opt/rust/cargo/bin (custom PATH). The ENV PATH
# above is inherited by non-login shells, but LOGIN shells (bash -l, e.g. the
# AIO terminal panel's pty) source /etc/profile which RESETS PATH to the
# standard set, dropping /opt/rust/cargo/bin -> `cargo` not found there.
# Symlink the proxies into /usr/local/bin, which is on PATH in EVERY shell
# (login / non-login / interactive / non-interactive), so cargo/rustc/rustup/
# rustfmt are universally findable with zero per-shell PATH management. The
# proxies are stable paths; rustup updates the toolchain behind them, so the
# symlinks stay valid across `rustup update`.
 && for b in /opt/rust/cargo/bin/*; do ln -sf "$b" /usr/local/bin/"$(basename "$b")"; done
# <<< scenario: rust <<<
