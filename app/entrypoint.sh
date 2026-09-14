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
# PI_WEB_ALLOWED_HOSTS mirrors /etc/profile.d/pi-web.sh (baked by the pi-web
# scenario) so the sandbox-net name http://app:30141 keeps working; this
# script does not run a login shell, so profile.d is NOT sourced here.
# Overridable since sandbox-mgr Phase 2 (design §2.1): the mgr-generated
# sandbox compose sets it to "app,sbx-<name>-piweb.mgr.localhost" (the total
# gateway passes the browser's Host through VERBATIM — no header_up rewrite,
# see mgr/src/caddy.rs — so this list is what actually admits the subdomain;
# the `*.localhost` suffix rule inside pi-web is the second belt).
# Unset => "app", the stock behavior.
#
# The entrypoint itself is NOT part of sandbox-base (it lives in the app
# image), so base rebuilds are unaffected.
#
# Logs go to ~/.aio/pi-web.log on the persistent workspace volume (visible from
# code-server / the terminal pane via `tail -f`), not to docker logs, which
# stays axum-only.
: "${HOME:=/root}"
if command -v pi-web >/dev/null 2>&1; then
	mkdir -p "$HOME/.aio"
	(
		while true; do
			PI_WEB_ALLOWED_HOSTS="${PI_WEB_ALLOWED_HOSTS:-app}" pi-web --no-open --hostname 0.0.0.0 --port 30141 \
				>>"$HOME/.aio/pi-web.log" 2>&1 || :
			sleep 2
		done
	) &
	echo "pi-web autostarted on 0.0.0.0:30141 (log: ~/.aio/pi-web.log)"
fi

# Redirect page target (unified Phase 6, D1): when MGR_URL is set this
# sandbox is managed by sandbox-mgr, so the static "/" page bounces the
# browser to the manager UI. mgr.localhost resolves inside aio-mgr-net
# (total gateway); the browser, however, reaches the manager from the HOST,
# so the literal public origin is substituted — the same URL the user types.
# MGR_URL itself is only the TRIGGER (its value is the container-internal
# model-pull endpoint, never browser-reachable); the substituted TARGET is
# MGR_REDIRECT_URL, defaulting to the canonical port-less origin — the
# redirect page's JS re-attaches the host port the browser actually used
# (app/redirect/index.html port-following). Unset (stock / pre-adopt stack):
# the placeholder stays and the page shows its static explanation instead of
# bouncing. sed -i on /app/static (not a bind mount; inode churn is
# irrelevant here, unlike caddy's Caddyfile).
# The current values (composegen default, this fallback) contain no sed
# replacement metacharacters, but MGR_REDIRECT_URL is operator-settable —
# escape `&` (otherwise it expands to the matched placeholder text) and
# `\` so an arbitrary URL substitutes verbatim.
MGR_REDIRECT_URL="${MGR_REDIRECT_URL:-http://mgr.localhost/}"
MGR_REDIRECT_ESC=$(printf '%s' "$MGR_REDIRECT_URL" | sed 's/[&\\]/\\&/g')
if [ -n "${MGR_URL:-}" ] && [ -f /app/static/index.html ]; then
	if sed -i "s|MGR_PLACEHOLDER_URL|${MGR_REDIRECT_ESC}|g" \
		/app/static/index.html 2>/dev/null; then
		echo "static / redirects to ${MGR_REDIRECT_URL} (MGR_URL set)"
	else
		echo "warn: could not patch /app/static/index.html redirect target"
	fi
fi

exec "$@"
