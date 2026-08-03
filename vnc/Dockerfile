# syntax=docker/dockerfile:1
#
# VNC / Chromium container - Phase G.
#
# DELIBERATELY NOT based on `sandbox-base`. vnc is a pure browser surface: it
# only needs to (a) read the shared workspace volume at /home/gem to open files
# built elsewhere, and (b) reach dev servers started in code-server / app via
# sandbox-net DNS (http://code-server:<port>). The dev tooling that lives in
# sandbox-base (node / python-dev / opencode / build-essential / uv / ruff /
# scenario fragments) is dead weight here - vnc never compiles or runs dev
# tools. Decoupling to debian:bookworm-slim sheds ~1.3GB and, just as
# importantly, insulates vnc from base/scenario rebuilds: adding a rust / go /
# python-dev scenario no longer stale-dates or rebuilds this image. See
# docs/offline-tool-install.md (cross-container section) for the dev->vnc
# preview workflow.
#
# What we DO replicate from Dockerfile.base.head (the minimum vnc needs):
#   - HTTPS apt sources + ca-certificates self-bootstrap (network policy blocks
#     plain HTTP port 80 - same chicken-and-egg as the base head).
#   - user `gem` (uid 1000), aligned with the shared workspace volume owner
#     (R6/R9). chromium runs as gem and writes its profile to
#     /home/gem/.config/chromium on the volume; a mismatched uid would break
#     profile persistence and cross-container file access.
#   - en_US.UTF-8 locale + tzdata (chromium server-side text rendering).
# `gem` is duplicated here vs Dockerfile.base.head - accepted tradeoff for the
# decoupling win; uid 1000 is stable, drift risk is low.
#
# Build context is the repo root (docker-compose sets `context: .` +
# `dockerfile: vnc/Dockerfile`), matching the `app` and `code-server` services.
#
# One container, four cooperating processes under a bash supervisor
# (vnc/entrypoint.sh) - design §7. websockify serves the noVNC web files at
# :6080 and proxies the WebSocket to the in-container Xvnc on 127.0.0.1:5900;
# caddy fronts the whole thing at /vnc/* (handle_path strips the prefix, so
# noVNC's relative asset URLs resolve correctly - same pattern as code-server).
# `shm_size: 2gb` is set in docker-compose (Chromium needs shared memory).
#
# noVNC: the debian `novnc` package (1.3.0) ships `vnc.html` (with relative
# asset URLs: src="app/ui.js", href="app/styles/base.css") at /usr/share/novnc,
# so no git clone is needed. websockify's `--web` points there.

FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=en_US.UTF-8 \
    LC_ALL=en_US.UTF-8 \
    TZ=UTC

# Force HTTPS apt sources (network policy blocks HTTP port 80), then bootstrap
# ca-certificates over HTTPS without peer verification (chicken-and-egg: HTTPS
# apt needs certs to verify, certs come from apt). Once installed, all
# subsequent HTTPS fetches verify normally. Mirrors Dockerfile.base.head.
RUN sed -i 's|http://deb.debian.org|https://deb.debian.org|g; s|http://security.debian.org|https://security.debian.org|g' \
        /etc/apt/sources.list.d/debian.sources 2>/dev/null || true \
 && sed -i 's|http://deb.debian.org|https://deb.debian.org|g; s|http://security.debian.org|https://security.debian.org|g' \
        /etc/apt/sources.list 2>/dev/null || true \
 && apt-get -o Acquire::https::Verify-Peer=false update \
 && apt-get -o Acquire::https::Verify-Peer=false install -y --no-install-recommends ca-certificates \
 && update-ca-certificates

# Install the VNC stack + a screenshot tool (ImageMagick `import`) for
# validation. All packages are in debian bookworm main, so a single apt-get
# install suffices. `novnc` (1.3.0) ships vnc.html with RELATIVE asset URLs, so
# the /vnc/ subpath works behind caddy handle_path (same pattern as code-server
# - design §3/§7). `websockify` pulls in python3 as a runtime dep (not dev
# tooling - no pip / venv / headers). `bash` for the entrypoint supervisor.
#
# `fonts-noto-cjk`: Chromium renders page text SERVER-SIDE (on the in-container
# X display, streamed via VNC), so unlike code-server (client-side render) the
# font must live in THIS container. Without a CJK font, Chinese/Japanese/Korean
# pages show tofu boxes (e.g. www.baidu.com). Noto CJK covers all three.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        bash \
        tigervnc-standalone-server \
        openbox \
        chromium \
        novnc \
        websockify \
        imagemagick \
        fonts-noto-cjk \
        locales \
        tzdata \
 && rm -rf /var/lib/apt/lists/* \
 && sed -i '/en_US.UTF-8/s/^# //g' /etc/locale.gen && locale-gen \
 && Xvnc -version \
 && websockify --help >/dev/null \
 && openbox --version \
 && chromium --version \
 && import -version | head -1 \
 && test -f /usr/share/novnc/vnc.html \
 && fc-list | grep -qi "noto.*cjk"

# Workspace user `gem` (uid 1000), aligned with the shared workspace volume
# owner (R6/R9). chromium runs as gem and writes its profile to
# /home/gem/.config/chromium on the volume. Duplicated from Dockerfile.base.head
# by design (see header) - vnc is decoupled from sandbox-base.
RUN groupadd --system --gid 1000 gem \
 && useradd --uid 1000 --gid 1000 --create-home --shell /bin/bash gem \
 && mkdir -p /home/gem \
 && chown 1000:1000 /home/gem

# Bash supervisor: starts Xvnc, openbox, chromium, websockify; reaps children
# via `wait -n` + an EXIT trap (no zombies). Chosen over s6-overlay/supervisord
# for MVP simplicity (implement.md allows the fallback).
COPY vnc/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

USER gem
WORKDIR /home/gem
EXPOSE 6080
CMD ["/entrypoint.sh"]
