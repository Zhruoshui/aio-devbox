#!/bin/bash
# VNC container supervisor (bash). Starts the four cooperating processes that
# make a Chromium desktop drivable from the browser (R3 / design §7), and keeps
# the container alive.
#
#   1. TigerVNC Xvnc        - X server on display :99, RFB on 127.0.0.1:5900
#                             (no auth - internal only, -localhost).
#   2. openbox              - window manager on :99.
#   3. chromium             - the browser, on :99 (anti-automation + container
#                             flags; profile on the shared volume).
#   4. websockify           - serves noVNC web files at :6080 + proxies the
#                             WebSocket to 127.0.0.1:5900 (long-running).
#
# Supervision model: `wait -n` blocks until ANY child exits, then the EXIT trap
# kills + reaps the rest (no zombies). If a critical process dies the whole
# container restarts via compose `restart: unless-stopped`, bringing the desktop
# back cleanly rather than running half-broken. A bash supervisor is used
# instead of s6-overlay/supervisord for MVP simplicity (implement.md risky-note
# allows the fallback). Runs as uid 1000 (gem) - inherited from the image.

set -u

export HOME=/home/gem
export DISPLAY=:99

XVNC_PID=""
OPENBOX_PID=""
CHROMIUM_PID=""
WS_PID=""

cleanup() {
  # Kill remaining children (best-effort) then reap so nothing is orphaned.
  for pid in "$CHROMIUM_PID" "$OPENBOX_PID" "$XVNC_PID" "$WS_PID"; do
    [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
  done
  wait 2>/dev/null || true
}
trap cleanup EXIT
# On INT/TERM (e.g. `docker stop`): exit cleanly so the EXIT trap runs.
trap 'exit 0' INT TERM

# Stale X lock cleanup (an unclean restart can leave /tmp/.X99-lock, which would
# make Xvnc refuse to start). /tmp is world-writable so gem can clean it.
rm -f /tmp/.X99.lock 2>/dev/null || true
rm -f /tmp/.X11-unix/X99 2>/dev/null || true
mkdir -p /tmp/.X11-unix /home/gem/.config/chromium

# Stale Chromium singleton-lock cleanup. Chromium writes SingletonLock /
# SingletonCookie / SingletonSocket (symlinks targeting <hostname>-<pid>) in the
# profile dir. An unclean shutdown - notably a container RECREATE, which gives
# the new container a different hostname - leaves these behind on the shared
# volume. On the next start Chromium sees a foreign-hostname lock, refuses to
# launch ("profile appears to be in use by another computer"), shows a dialog,
# and exits; `wait -n` then tears the whole container down -> crash loop. At
# entrypoint start no Chromium is running in this container (we launch the only
# one below), so removing them unconditionally is safe and lets Chromium reuse
# the persisted profile (AC3) across restarts AND recreates.
rm -f /home/gem/.config/chromium/SingletonLock \
      /home/gem/.config/chromium/SingletonCookie \
      /home/gem/.config/chromium/SingletonSocket 2>/dev/null || true

# 1. TigerVNC Xvnc on display :99. -rfbport 5900 overrides the 5900+display
#    convention; -SecurityTypes None = no VNC password (internal only);
#    -localhost binds the RFB socket to 127.0.0.1 (websockify connects locally).
Xvnc :99 \
  -geometry 1280x800 \
  -depth 24 \
  -rfbport 5900 \
  -SecurityTypes None \
  -localhost &
XVNC_PID=$!

# Wait for the X server socket before starting X clients (openbox/chromium).
for _ in $(seq 1 50); do
  [ -S /tmp/.X11-unix/X99 ] && break
  sleep 0.1
done
if [ ! -S /tmp/.X11-unix/X99 ]; then
  echo "entrypoint: Xvnc did not start (no /tmp/.X11-unix/X99)" >&2
  exit 1
fi

# 2. openbox window manager (--sm-disable = no session manager, simpler startup).
openbox --sm-disable &
OPENBOX_PID=$!

# 3. Chromium. --no-sandbox is required in containers (no setuid sandbox helper);
#    --disable-dev-shm-usage avoids /dev/shm exhaustion (shm_size is also 2gb);
#    --user-data-dir keeps the profile on the shared workspace volume (R6/AC3).
chromium \
  --no-sandbox \
  --disable-dev-shm-usage \
  --disable-gpu \
  --no-first-run \
  --no-default-browser-check \
  --disable-features=TranslateUI \
  --user-data-dir=/home/gem/.config/chromium \
  about:blank &
CHROMIUM_PID=$!

# 4. websockify: serve noVNC web files at 0.0.0.0:6080 and proxy the WebSocket
#    to the in-container Xvnc on 127.0.0.1:5900. The long-running foreground
#    process; noVNC's vnc.html connects to ws://<gateway>/vnc/websockify.
websockify --web=/usr/share/novnc 0.0.0.0:6080 localhost:5900 &
WS_PID=$!

# Block until any child exits, then tear down (the container restarts whole via
# compose `restart: unless-stopped`). Reaps the first zombie; EXIT trap reaps
# the rest.
wait -n 2>/dev/null || true
echo "entrypoint: a supervised process exited; shutting down" >&2
