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

USER gem
EXPOSE 8200
# `--auth none` because caddy enforces basicauth at the gateway (design §6/§10).
# --user-data-dir is inside the shared workspace volume so settings/extensions
# persist across restarts (R6 / AC3).
CMD ["code-server", \
     "--host", "0.0.0.0", \
     "--port", "8200", \
     "--auth", "none", \
     "--user-data-dir", "/home/gem/.local/share/code-server"]
