# PoC only (task #3, Phase B): bake mise as a build-time install engine.
# Simulates what scenarios/mise + a mise-managed rust/opencode preset would
# produce. NOT for merge — disposable proof, lives in mise-poc/.

FROM sandbox-base

ARG MISE_VERSION=v2026.9.0
ARG RUST_VERSION=1.93.1
ARG OPENCODE_VERSION=1.18.24

# ── Layer 1: mise binary itself, system path ─────────────────────────────
# (the "manager" half of the nvm/uv hybrid pattern: binary baked system-side)
RUN curl -fsSL "https://github.com/jdx/mise/releases/download/${MISE_VERSION}/mise-${MISE_VERSION}-linux-x64.tar.gz" -o /tmp/mise.tar.gz \
 && tar -xzf /tmp/mise.tar.gz -C /tmp \
 && install -m 0755 /tmp/mise/bin/mise /usr/local/bin/mise \
 && rm -rf /tmp/mise /tmp/mise.tar.gz \
 && mise --version

# ── Layer 2: tools baked via mise at build time, ALL on system paths ─────
# Two masking hazards found in Phase B round 1:
#   1. MISE_DATA_DIR defaults to ~/.local/share/mise (masked by the volume)
#      → force /opt/mise.
#   2. mise core:rust is a rustup ADOPTER: the real 1.4G toolchain goes to
#      RUSTUP_HOME (~/.rustup default — masked!) and installs/rust/<v> is
#      just a symlink into ~/.cargo/bin. At runtime the symlink dangles and
#      mise auto_install silently re-downloads the toolchain into the volume.
#      → force RUSTUP_HOME/CARGO_HOME onto system paths too (same shape as
#      the existing scenarios/rust fragment).
ENV MISE_DATA_DIR=/opt/mise \
    RUSTUP_HOME=/opt/mise/rustup \
    CARGO_HOME=/opt/mise/cargo

RUN mkdir -p /opt/mise \
 && mise use --global rust@${RUST_VERSION} \
# core:rust ships a default profile WITHOUT rust-analyzer (Phase A finding):
# without it the rustup proxy falls back into the mise shim → infinite
# recursion. Pre-add the component.
 && mise exec -- rustup component add rust-analyzer \
 && mise use --global opencode@${OPENCODE_VERSION} \
 && echo "=== baked tools ===" \
 && mise ls \
 && du -sh /opt/mise \
 && echo "=== installs are REAL files (not symlinks into /root) ===" \
 && ls -la /opt/mise/installs/rust/${RUST_VERSION}/ | head -5

# ── Layer 3: login-shell activation ──────────────────────────────────────
# Rule 2: bash -l resets PATH → /etc/profile.d. Repoint MISE_DATA_DIR and
# the rust homes so runtime shells see exactly what was baked (shims live at
# /opt/mise/shims).
RUN printf '%s\n' \
      '# mise activation for login shells (bash -l).' \
      'export MISE_DATA_DIR=/opt/mise' \
      'export RUSTUP_HOME=/opt/mise/rustup' \
      'export CARGO_HOME=/opt/mise/cargo' \
      'eval "$(mise activate bash)"' \
      > /etc/profile.d/mise.sh

# ── verify: every command reachable in a LOGIN shell ─────────────────────
RUN bash -lc 'mise --version' \
 && bash -lc 'rustc --version && cargo --version && rustfmt --version && rust-analyzer --version' \
 && bash -lc 'opencode --version'
