# syntax=docker/dockerfile:1
#
# code-server (VSCode in the browser) - Phase D.
#
# Derives from the shared `sandbox-base` image (built by `make build-base`),
# which already carries node / python / git / opencode, runs as root and has
# WORKDIR /root. We only add code-server on top.
#
# Build context is the repo root (docker-compose sets `context: .` +
# `dockerfile: code-server/Dockerfile`), matching the `app` service.
#
# Install method: code-server.dev is blocked by this sandbox's network policy
# (HTTP 403), so we fetch the pinned `.deb` straight from GitHub releases -
# the same fallback pattern Dockerfile.base uses for node/opencode. `dpkg -i`
# installs cleanly with no missing dependencies on sandbox-base (verified).
# Installs to /usr/bin/code-server.
#
# Subpath note: code-server has NO --prefix / base-path flag. It is served
# behind the /code-server/ subpath via caddy `handle_path` (strip prefix),
# which works because code-server emits all-relative asset URLs and computes
# its base URL from window.location. See gateway/Caddyfile + design §3/§6.

ARG CODE_SERVER_VERSION=4.130.0
# Global build arg: lets CI's buildx (docker-container driver) point the base
# stage at a GHCR-hosted sandbox-base. Defaults to local `sandbox-base`, so
# local builds are unchanged. Declared before FROM so this stage sees it.
ARG BASE_IMAGE=sandbox-base

FROM ${BASE_IMAGE}

ARG CODE_SERVER_VERSION

# sandbox-base's default user is root; install system-wide.
RUN curl -fsSL "https://github.com/coder/code-server/releases/download/v${CODE_SERVER_VERSION}/code-server_${CODE_SERVER_VERSION}_amd64.deb" \
        -o /tmp/code-server.deb \
 && dpkg -i /tmp/code-server.deb \
 && rm /tmp/code-server.deb \
 && code-server --version

# Pre-install VS Code extensions for code-server at BUILD time, fetched from
# Open VSX (openvsx.org reachable from the build host; verified). They land in
# /opt/cs-extensions - deliberately OUTSIDE /root, because the workspace
# volume mounts over /root at runtime and would mask anything baked under
# it (same volume-mask rule as the rust scenario's /opt/rust). Root-owned is
# fine: code-server runs as root, so `code-server --install-extension` at
# runtime still writes into the same dir. CAVEAT: those runtime installs land
# in the CONTAINER WRITABLE LAYER (not a volume) and are LOST on container
# recreate - only the baked list below persists. Treat runtime installs as
# trials; promote the keepers into this list and rebuild. The runtime CMD
# below points --extensions-dir here so code-server loads these pre-installed
# extensions.
#
# To add/remove an extension: edit the list below, then re-run `make build`.
#
# MINIMAL CORE SET (issue #9): only extensions that close a loop with a
# scenario baked into sandbox-base (LSP / debug / lint-format) or with this
# repo's own config formats. Everything else - language packs for stacks
# absent from the image (Vue/Tailwind/Jupyter-without-a-kernel), cosmetics,
# git UI - was removed as dead weight. Trial installs at runtime are the right
# place for taste-driven extensions. Each entry carries a `# ←` comment naming
# what it is aligned to, so the list cannot silently drift from the scenario
# system again (that drift is how this issue started).
#
# IMPORTANT license notes (issue #9):
#   - code-server uses the Open VSX registry, NOT the Microsoft marketplace.
#   - ms-vscode.cpptools is UNUSABLE in code-server regardless of how it is
#     installed: its runtime license check rejects non-MS hosts ("may be used
#     only with Microsoft Visual Studio, Visual Studio Code...") and the
#     language server exits immediately. Vendoring its .vsix does NOT bypass
#     this - do not reintroduce it. clangd (Open VSX + the binary baked by
#     the c23 scenario) is the C/C++ path.
#   - Python: Pylance is license-restricted the same way; ruff + debugpy +
#     the python ext cover lint/format/debug/autocomplete instead.
#   - golang.go deliberately NOT installed: its gopls auto-download at first
#     activation takes ~40s and lands in the workspace volume; if Go support
#     is wanted, bake gopls into scenarios/go first (issue #9 option (b)).
RUN mkdir -p /opt/cs-extensions
RUN set -e; \
    # Each entry below carries an aligned-to comment (a scenario baked into
    # sandbox-base, or this repo's config formats) so the list cannot silently
    # drift from the scenario system again - see issue #9. Lines starting with
    # '#' are stripped by the Dockerfile parser even inside continuations.
    for ext in \
      # <- rust scenario (rustup component, symlinked /usr/local/bin)
      rust-lang.rust-analyzer \
      # <- repo config format (scenario/services/enabled.toml)
      tamasfe.even-better-toml \
      # <- python always_on
      ms-python.python \
      # <- python always_on (debug)
      ms-python.debugpy \
      # <- python-dev scenario (/usr/local/bin/ruff)
      charliermarsh.ruff \
      # <- c23 scenario (clangd-22; cpptools is license-blocked in code-server)
      llvm-vs-code-extensions.vscode-clangd \
      # <- c23 scenario (clang-format-22)
      xaver.clang-format \
      # <- c23 scenario (cmake/ninja)
      twxs.cmake \
      # <- repo config format (compose/CI/trellis)
      redhat.vscode-yaml \
    ; do \
      echo ">>> installing $ext"; \
      code-server --install-extension "$ext" \
        --extensions-dir /opt/cs-extensions \
        --user-data-dir /tmp/cs-build; \
    done; \
    rm -rf /tmp/cs-build

EXPOSE 8200
# `--auth none` because caddy enforces basicauth at the gateway (design §6/§10).
# --user-data-dir is inside the shared workspace volume so settings persist
# across restarts (R6 / AC3). --extensions-dir points at the image-baked
# /opt/cs-extensions (outside the volume) so pre-installed extensions survive
# container recreation and are not masked by the volume.
CMD ["code-server", \
     "--host", "0.0.0.0", \
     "--port", "8200", \
     "--auth", "none", \
     "--user-data-dir", "/root/.local/share/code-server", \
     "--extensions-dir", "/opt/cs-extensions"]
