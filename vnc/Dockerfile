# syntax=docker/dockerfile:1
#
# VNC / Chromium container - Phase G.
#
# Derives from the shared `sandbox-base` image (built by `make build-base`),
# which already carries node / python / git / opencode + user `gem` (uid 1000)
# and WORKDIR /home/gem, with apt forced to HTTPS. We only add the
# noVNC-over-websockify + TigerVNC Xvnc + openbox + Chromium stack on top.
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

FROM sandbox-base

USER root

# apt HTTPS sources are already configured in sandbox-base. Install the VNC
# stack + a screenshot tool (ImageMagick `import`) for validation. All five
# packages are in debian bookworm main, so a single apt-get install suffices.
# `novnc` (1.3.0) ships vnc.html with RELATIVE asset URLs, so the /vnc/ subpath
# works behind caddy handle_path (same pattern as code-server - design §3/§7).
#
# `fonts-noto-cjk`: Chromium renders page text SERVER-SIDE (on the in-container
# X display, streamed via VNC), so unlike code-server (client-side render) the
# font must live in THIS container. Without a CJK font, Chinese/Japanese/Korean
# pages show tofu boxes (e.g. www.baidu.com). Noto CJK covers all three. Put
# here, not in sandbox-base, because app/code-server don't server-render text.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        tigervnc-standalone-server \
        openbox \
        chromium \
        novnc \
        websockify \
        imagemagick \
        fonts-noto-cjk \
 && rm -rf /var/lib/apt/lists/* \
 && Xvnc -version \
 && websockify --help >/dev/null \
 && openbox --version \
 && chromium --version \
 && import -version | head -1 \
 && test -f /usr/share/novnc/vnc.html \
 && fc-list | grep -qi "noto.*cjk"

# Bash supervisor: starts Xvnc, openbox, chromium, websockify; reaps children
# via `wait -n` + an EXIT trap (no zombies). Chosen over s6-overlay/supervisord
# for MVP simplicity (implement.md allows the fallback).
COPY vnc/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

USER gem
EXPOSE 6080
CMD ["/entrypoint.sh"]
