#!/bin/bash
# Wrapper: route agent-browser to the VNC chromium via CDP (localhost:9222,
# shared netns). Without it agent-browser launches its own headless Chrome —
# invisible in VNC, not sharing cookies/state with manual browsing (R3/AC5).
# Real binary baked as agent-browser-real by fragment.Dockerfile; this wrapper
# is COPY'd on top as /usr/local/bin/agent-browser.
#
# Escape hatches (R7): pass `--cdp <port>` yourself → passthrough untouched;
# or call agent-browser-real directly to bypass this wrapper entirely.
# Browser backend comes from the vnc scenario; when vnc is off, browser-class
# subcommands fail with an actionable error (R4), whitelist below still runs.

set -u

CDP_PORT="${BROWSER_CDP_PORT:-9222}"
SELF="$(readlink -f "$0" 2>/dev/null || printf '%s' "$0")"

# Resolve real binary: agent-browser-real (postinstall→native Rust binary);
# fall back to the node launcher if the rename step didn't happen.
resolve_real() {
  local real js
  real="$(command -v agent-browser-real 2>/dev/null || true)"
  if [ -n "$real" ]; then
    real="$(readlink -f "$real" 2>/dev/null || printf '%s' "$real")"
    if [ -x "$real" ] && [ "$real" != "$SELF" ]; then
      printf '%s\n' "$real"; return 0
    fi
  fi
  for js in \
    /usr/local/lib/node_modules/agent-browser/bin/agent-browser.js \
    "$(npm root -g 2>/dev/null)/agent-browser/bin/agent-browser.js"; do
    [ -f "$js" ] && { printf '%s\n' "$js"; return 0; }
  done
  return 1
}

REAL="$(resolve_real || true)"
[ -n "$REAL" ] || { echo "ERROR: agent-browser real binary not found" >&2; exit 1; }

# Escape hatch (R7): user already passed --cdp → don't inject again.
for arg in "$@"; do [ "$arg" = "--cdp" ] && exec "$REAL" "$@"; done

# Whitelist: subcommands/flags that never need a running browser backend.
case "${1:-}" in
  version|help|doctor|install|upgrade|config|completions) exec "$REAL" "$@" ;;
esac
for arg in "$@"; do
  case "$arg" in --help|-h|--version|-V) exec "$REAL" "$@" ;; esac
done

# Probe CDP (shared netns localhost → vnc chromium, bound to 127.0.0.1).
if ! curl -s --max-time 1 "http://127.0.0.1:${CDP_PORT}/json/version" >/dev/null 2>&1; then
  cat >&2 <<EOF
ERROR: VNC chromium CDP (localhost:${CDP_PORT}) 不可达。agent-browser 的浏览器后端由
vnc 场景提供。请确认:
  1. vnc 场景已启用 (make config 勾选 vnc);
  2. aio-vnc-1 在运行 (docker start aio-vnc-1);
或直调 agent-browser-real 走自管模式 (绕过本 wrapper, 启动独立 headless 浏览器)。
EOF
  exit 1
fi

exec "$REAL" --cdp "${CDP_PORT}" "$@"
