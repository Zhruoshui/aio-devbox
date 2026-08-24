# syntax=docker/dockerfile:1
#
# code-server (VSCode in the browser) - Phase D.
#
# Derives from the shared `sandbox-base` image (built by `make build-base`),
# which already carries node / python / git / opencode + user `gem` (uid 1000)
# and WORKDIR /home/gem. We only add code-server on top.
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

FROM sandbox-base

ARG CODE_SERVER_VERSION

# sandbox-base ends with `USER gem`; install system-wide as root, then drop back.
USER root
RUN curl -fsSL "https://github.com/coder/code-server/releases/download/v${CODE_SERVER_VERSION}/code-server_${CODE_SERVER_VERSION}_amd64.deb" \
        -o /tmp/code-server.deb \
 && dpkg -i /tmp/code-server.deb \
 && rm /tmp/code-server.deb \
 && code-server --version

# Pre-install VS Code extensions for code-server at BUILD time, fetched from
# Open VSX (openvsx.org reachable from the build host; verified). They land in
# /opt/cs-extensions - deliberately OUTSIDE /home/gem, because the workspace
# volume mounts over /home/gem at runtime and would mask anything baked under
# it (same volume-mask rule as the rust scenario's /opt/rust). Chowned to gem
# (uid 1000) so the dev can still `code-server --install-extension` at runtime
# into the same dir. CAVEAT: those runtime installs land in the CONTAINER
# WRITABLE LAYER (not a volume) and are LOST on container recreate - only the
# baked list below persists. Treat runtime installs as trials; promote the
# keepers into this list and rebuild. The runtime CMD below points
# --extensions-dir here so code-server loads these pre-installed extensions.
#
# To add/remove an extension: edit the list below, then re-run `make build`.
#
# IMPORTANT: code-server uses the Open VSX registry, NOT the Microsoft
# marketplace, so license-restricted MS extensions are unavailable via Open VSX
# and must be vendored as a .vsix from GitHub releases (same pattern as the
# code-server .deb / node tarball above):
#   - C/C++ IntelliSense: ms-vscode.cpptools is absent from Open VSX -> fetched
#     as cpptools-linux-x64.vsix below. (clangd extension is also absent.) The
#     .vsix bundles its own native language server (cpptools/cpptools-srv), so
#     no extra system deps are needed.
#   - Python: Pylance (ms-python.vscode-pylance) is absent; ruff + debugpy +
#     the python ext cover lint/format/debug/autocomplete instead.
ARG CPPTOOLS_VERSION=1.33.8
RUN mkdir -p /opt/cs-extensions && chown -R 1000:1000 /opt/cs-extensions
USER gem
RUN set -e; \
    for ext in \
      rust-lang.rust-analyzer \
      serayuzgur.crates \
      tamasfe.even-better-toml \
      twxs.cmake \
      ms-vscode.cmake-tools \
      xaver.clang-format \
      ms-python.python \
      ms-python.debugpy \
      charliermarsh.ruff \
      ms-toolsai.jupyter \
      Vue.volar \
      dbaeumer.vscode-eslint \
      esbenp.prettier-vscode \
      bradlc.vscode-tailwindcss \
      antfu.vite \
      steoates.autoimport \
      EditorConfig.EditorConfig \
      eamodio.gitlens \
      mhutchie.git-graph \
      vscode-icons-team.vscode-icons \
      oderwat.indent-rainbow \
      aaron-bond.better-comments \
      ms-vscode.live-server \
      redhat.vscode-yaml \
    ; do \
      echo ">>> installing $ext"; \
      code-server --install-extension "$ext" \
        --extensions-dir /opt/cs-extensions \
        --user-data-dir /tmp/cs-build; \
    done; \
    echo ">>> installing ms-vscode.cpptools (vendored .vsix v${CPPTOOLS_VERSION})"; \
    curl -fsSL -o /tmp/cpptools.vsix \
        "https://github.com/microsoft/vscode-cpptools/releases/download/v${CPPTOOLS_VERSION}/cpptools-linux-x64.vsix"; \
    code-server --install-extension /tmp/cpptools.vsix \
        --extensions-dir /opt/cs-extensions \
        --user-data-dir /tmp/cs-build; \
    rm -rf /tmp/cpptools.vsix /tmp/cs-build

USER gem
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
     "--user-data-dir", "/home/gem/.local/share/code-server", \
     "--extensions-dir", "/opt/cs-extensions"]
