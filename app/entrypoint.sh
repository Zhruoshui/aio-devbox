#!/bin/sh
set -eu

# App container entrypoint: autostart pi-web (if baked), then exec the server.
#
# pi-web used to be launched on demand by the "pi Web" pty button. It is now a
# type=web iframe pane (services.toml), which requires the server to already be
# listening when the button's TCP probe runs - so it is started here, at
# container boot, in a respawn loop.
#
# Why a loop instead of a bare background start: pi-web binds 0.0.0.0:30141 on
# app's netns (shared with code-server/vnc). If it dies (crash, or a manual
# `pi-web` run in a terminal grabbed the port and later exited), the pane would
# go dark until a container recreate. The loop retries every 2s; while a manual
# instance holds the port the autostart attempt just fails EADDRINUSE into the
# log and backs off - the manual instance serves the pane meanwhile.
#
# Conditional on `command -v`: the pi-web scenario is optional. When it isn't
# baked, nothing listens on 30141, the manifest probe fails, and the button
# hides itself (same degradation the old command_exists probe gave).
#
# PI_WEB_ALLOWED_HOSTS=app mirrors /etc/profile.d/pi-web.sh (baked by the
# pi-web scenario) so the sandbox-net name http://app:30141 keeps working; this
# script does not run a login shell, so profile.d is NOT sourced here. The
# entrypoint itself is NOT part of sandbox-base (it lives in the app image), so
# base rebuilds are unaffected.
#
# Logs go to ~/.aio/pi-web.log on the persistent workspace volume (visible from
# code-server / the terminal pane via `tail -f`), not to docker logs, which
# stays axum-only.
: "${HOME:=/root}"
if command -v pi-web >/dev/null 2>&1; then
	mkdir -p "$HOME/.aio"
	(
		while true; do
			PI_WEB_ALLOWED_HOSTS=app pi-web --no-open --hostname 0.0.0.0 --port 30141 \
				>>"$HOME/.aio/pi-web.log" 2>&1 || :
			sleep 2
		done
	) &
	echo "pi-web autostarted on 0.0.0.0:30141 (log: ~/.aio/pi-web.log)"
fi

exec "$@"
